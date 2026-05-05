#!/usr/bin/env python3
from __future__ import annotations

import argparse
import dataclasses
import datetime as dt
import html as html_lib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import textwrap
import tomllib
import xml.etree.ElementTree as ET
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
POSTS_DIR = ROOT / "posts"
ASSETS_DIR = ROOT / "assets"
PUBLIC_DIR = ROOT / "public"
POST_META_SELECTOR = "<post-meta>"


@dataclasses.dataclass(frozen=True)
class SiteConfig:
    title: str
    description: str
    author: str
    language: str
    base_url: str


@dataclasses.dataclass(frozen=True)
class Post:
    source: Path
    slug: str
    title: str
    date: dt.date
    description: str
    tags: tuple[str, ...]
    draft: bool


def main() -> int:
    parser = argparse.ArgumentParser(description="Build the Typst-powered static blog.")
    parser.add_argument("--site-dir", type=Path, default=ROOT / "_site")
    parser.add_argument("--dist-dir", type=Path, default=ROOT / "dist")
    parser.add_argument(
        "--check",
        action="store_true",
        help="Build into temporary directories to validate the whole pipeline.",
    )
    args = parser.parse_args()

    site_config = read_site_config()
    if args.check:
        with tempfile.TemporaryDirectory(prefix="blog-check-") as tmp:
            tmp_root = Path(tmp)
            build(site_config, tmp_root / "_site", tmp_root / "dist")
    else:
        build(site_config, args.site_dir, args.dist_dir)

    return 0


def read_site_config() -> SiteConfig:
    config_path = ROOT / "site.toml"
    data = tomllib.loads(config_path.read_text(encoding="utf-8"))
    base_url = os.environ.get("SITE_URL") or data.get("base_url", "")
    base_url = str(base_url).rstrip("/")
    return SiteConfig(
        title=require_str(data, "title", config_path),
        description=require_str(data, "description", config_path),
        author=require_str(data, "author", config_path),
        language=require_str(data, "language", config_path),
        base_url=base_url,
    )


def build(site: SiteConfig, site_dir: Path, dist_dir: Path) -> None:
    posts = discover_posts()
    published_posts = [post for post in posts if not post.draft]
    published_posts.sort(key=lambda post: (post.date, post.slug), reverse=True)

    clean_dir(site_dir)
    clean_dir(dist_dir)
    (site_dir / "posts").mkdir(parents=True, exist_ok=True)
    (site_dir / "downloads").mkdir(parents=True, exist_ok=True)
    (dist_dir / "md").mkdir(parents=True, exist_ok=True)

    copy_tree_if_exists(ASSETS_DIR, site_dir / "assets")
    copy_tree_if_exists(PUBLIC_DIR, site_dir)

    for post in published_posts:
        article_html = render_typst_html(post)
        markdown = render_markdown(post, site)

        post_dir = site_dir / "posts" / post.slug
        post_dir.mkdir(parents=True, exist_ok=True)
        write_text(post_dir / "index.html", render_post_page(site, post, article_html))
        write_text(site_dir / "downloads" / f"{post.slug}.md", markdown)
        write_text(dist_dir / "md" / f"{post.slug}.md", markdown)

    write_text(site_dir / "index.html", render_index_page(site, published_posts))
    write_text(site_dir / "feed.xml", render_feed(site, published_posts))
    write_text(site_dir / "robots.txt", render_robots(site))
    write_text(site_dir / ".nojekyll", "")
    if site.base_url:
        write_text(site_dir / "sitemap.xml", render_sitemap(site, published_posts))

    print(f"Built {len(published_posts)} post(s) into {display_path(site_dir)}")
    print(f"Wrote Markdown distribution files into {display_path(dist_dir / 'md')}")


def discover_posts() -> list[Post]:
    post_files = sorted(POSTS_DIR.glob("*.typ"))
    if not post_files:
        raise BuildError(f"No Typst posts found in {POSTS_DIR}")

    posts = [read_post(path) for path in post_files]
    slugs: dict[str, Path] = {}
    for post in posts:
        if post.slug in slugs:
            raise BuildError(f"Duplicate post slug {post.slug!r}: {slugs[post.slug]} and {post.source}")
        slugs[post.slug] = post.source
    return posts


def read_post(path: Path) -> Post:
    completed = run(
        [
            "typst",
            "query",
            "--features",
            "html",
            "--target",
            "html",
            "--root",
            str(ROOT),
            str(path),
            POST_META_SELECTOR,
            "--one",
        ],
        cwd=ROOT,
        action=f"read metadata from {path.relative_to(ROOT)}",
    )
    payload = json.loads(completed.stdout)
    meta = payload.get("value")
    if not isinstance(meta, dict):
        raise BuildError(f"{path.relative_to(ROOT)} has invalid {POST_META_SELECTOR} metadata")

    slug = require_str(meta, "slug", path)
    if not re.fullmatch(r"[a-z0-9]+(?:-[a-z0-9]+)*", slug):
        raise BuildError(f"{path.relative_to(ROOT)} slug must be lowercase kebab-case ASCII: {slug!r}")

    return Post(
        source=path,
        slug=slug,
        title=require_str(meta, "title", path),
        date=parse_date(require_str(meta, "date", path), path),
        description=require_str(meta, "description", path),
        tags=tuple(read_tags(meta, path)),
        draft=bool(meta.get("draft", False)),
    )


def render_typst_html(post: Post) -> str:
    with tempfile.TemporaryDirectory(prefix="typst-html-") as tmp:
        output = Path(tmp) / f"{post.slug}.html"
        run(
            [
                "typst",
                "compile",
                "--features",
                "html",
                "--format",
                "html",
                "--root",
                str(ROOT),
                str(post.source),
                str(output),
            ],
            cwd=ROOT,
            action=f"compile {post.source.relative_to(ROOT)} to HTML",
        )
        body = extract_body(output.read_text(encoding="utf-8")).strip()
        return strip_duplicate_title(body, post.title)


def render_markdown(post: Post, site: SiteConfig) -> str:
    source = strip_post_metadata(post.source.read_text(encoding="utf-8"))
    body = render_markdown_with_pandoc(post, source)
    if body is None:
        body = render_markdown_fallback(source)

    source_url = absolute_url(site, f"posts/{post.slug}/")
    frontmatter = [
        "---",
        f'title: "{yaml_quote(post.title)}"',
        f'date: "{post.date.isoformat()}"',
        "tags:",
    ]
    if post.tags:
        frontmatter.extend(f'  - "{yaml_quote(tag)}"' for tag in post.tags)
    else:
        frontmatter.append("  []")
    if source_url:
        frontmatter.append(f'source: "{yaml_quote(source_url)}"')
    frontmatter.append("---")
    return "\n".join(frontmatter) + "\n\n" + body.strip() + "\n"


def render_markdown_with_pandoc(post: Post, source: str) -> str | None:
    if shutil.which("pandoc") is None:
        print("pandoc not found; using the limited Typst-to-Markdown fallback", file=sys.stderr)
        return None

    with tempfile.TemporaryDirectory(prefix="typst-md-") as tmp:
        output_path = Path(tmp) / f"{post.slug}.md"
        completed = run(
            [
                "pandoc",
                "--from",
                "typst",
                "--to",
                "gfm",
                "--wrap",
                "none",
                "--resource-path",
                str(ROOT),
                "-",
                "--output",
                str(output_path),
            ],
            cwd=post.source.parent,
            action=f"convert {post.source.relative_to(ROOT)} to Markdown",
            input_text=source,
        )
        _ = completed
        return output_path.read_text(encoding="utf-8")


def render_markdown_fallback(source: str) -> str:
    converted: list[str] = []
    for line in source.splitlines():
        match = re.match(r"^(=+)\s+(.*)$", line)
        if match:
            converted.append("#" * len(match.group(1)) + " " + match.group(2))
        else:
            converted.append(line)
    return "\n".join(converted).strip() + "\n"


def render_index_page(site: SiteConfig, posts: list[Post]) -> str:
    items = "\n".join(
        textwrap.dedent(
            f"""
            <li>
              <h2><a href="posts/{url_escape(post.slug)}/">{escape(post.title)}</a></h2>
              <time datetime="{post.date.isoformat()}">{format_date(post.date)}</time>
              <p>{escape(post.description)}</p>
            </li>
            """
        ).strip()
        for post in posts
    )
    if not items:
        items = "<li>暂无文章。</li>"
    return render_shell(
        site,
        title=site.title,
        description=site.description,
        depth=0,
        body=textwrap.dedent(
            f"""
            <main>
              <h1>{escape(site.title)}</h1>
              <p>{escape(site.description)}</p>
              <ul class="post-list">
                {items}
              </ul>
            </main>
            """
        ),
    )


def render_post_page(site: SiteConfig, post: Post, article_html: str) -> str:
    tags = " ".join(f"<span>#{escape(tag)}</span>" for tag in post.tags)
    return render_shell(
        site,
        title=f"{post.title} | {site.title}",
        description=post.description,
        depth=2,
        canonical_path=f"posts/{post.slug}/",
        body=textwrap.dedent(
            f"""
            <main>
              <article>
                <header>
                  <h1 class="post-title">{escape(post.title)}</h1>
                  <div class="post-meta">
                    <time datetime="{post.date.isoformat()}">{format_date(post.date)}</time>
                    {tags}
                  </div>
                </header>
                <div class="post-body">
                  {article_html}
                </div>
                <nav class="post-actions" aria-label="文章操作">
                  <a href="../../">返回首页</a>
                  <a href="../../downloads/{url_escape(post.slug)}.md">Markdown 版本</a>
                </nav>
              </article>
            </main>
            """
        ),
    )


def render_shell(
    site: SiteConfig,
    *,
    title: str,
    description: str,
    depth: int,
    body: str,
    canonical_path: str = "",
) -> str:
    prefix = "../" * depth
    home_href = prefix or "./"
    canonical = absolute_url(site, canonical_path) if canonical_path else site.base_url
    canonical_tag = f'<link rel="canonical" href="{escape_attr(canonical)}">' if canonical else ""
    return textwrap.dedent(
        f"""\
        <!doctype html>
        <html lang="{escape_attr(site.language)}">
        <head>
          <meta charset="utf-8">
          <meta name="viewport" content="width=device-width, initial-scale=1">
          <title>{escape(title)}</title>
          <meta name="description" content="{escape_attr(description)}">
          {canonical_tag}
          <link rel="alternate" type="application/rss+xml" title="{escape_attr(site.title)}" href="{prefix}feed.xml">
          <link rel="stylesheet" href="{prefix}assets/style.css">
        </head>
        <body>
          <header class="site-header">
            <p class="site-title"><a href="{home_href}">{escape(site.title)}</a></p>
            <p class="site-description">{escape(site.description)}</p>
          </header>
        {body.rstrip()}
          <footer class="site-footer">
            <span>{escape(site.author)}</span>
          </footer>
        </body>
        </html>
        """
    ).lstrip()


def render_feed(site: SiteConfig, posts: list[Post]) -> str:
    channel = ET.Element("channel")
    ET.SubElement(channel, "title").text = site.title
    ET.SubElement(channel, "description").text = site.description
    ET.SubElement(channel, "link").text = site.base_url or "."
    ET.SubElement(channel, "language").text = site.language
    latest = posts[0].date if posts else dt.date.today()
    ET.SubElement(channel, "lastBuildDate").text = rfc2822(
        dt.datetime.combine(latest, dt.time.min, tzinfo=dt.timezone.utc)
    )

    for post in posts[:20]:
        item = ET.SubElement(channel, "item")
        post_url = absolute_url(site, f"posts/{post.slug}/") or f"posts/{post.slug}/"
        ET.SubElement(item, "title").text = post.title
        ET.SubElement(item, "description").text = post.description
        ET.SubElement(item, "link").text = post_url
        ET.SubElement(item, "guid").text = post_url
        ET.SubElement(item, "pubDate").text = rfc2822(
            dt.datetime.combine(post.date, dt.time.min, tzinfo=dt.timezone.utc)
        )

    rss = ET.Element("rss", version="2.0")
    rss.append(channel)
    return '<?xml version="1.0" encoding="utf-8"?>\n' + ET.tostring(rss, encoding="unicode") + "\n"


def render_sitemap(site: SiteConfig, posts: list[Post]) -> str:
    urlset = ET.Element("urlset", xmlns="http://www.sitemaps.org/schemas/sitemap/0.9")
    for path, modified in [("", None)] + [(f"posts/{post.slug}/", post.date) for post in posts]:
        url = ET.SubElement(urlset, "url")
        ET.SubElement(url, "loc").text = absolute_url(site, path)
        if modified:
            ET.SubElement(url, "lastmod").text = modified.isoformat()
    return '<?xml version="1.0" encoding="utf-8"?>\n' + ET.tostring(urlset, encoding="unicode") + "\n"


def render_robots(site: SiteConfig) -> str:
    lines = ["User-agent: *", "Allow: /"]
    if site.base_url:
        lines.append(f"Sitemap: {site.base_url}/sitemap.xml")
    return "\n".join(lines) + "\n"


def extract_body(document: str) -> str:
    match = re.search(r"<body>\s*(.*?)\s*</body>", document, flags=re.DOTALL | re.IGNORECASE)
    if not match:
        raise BuildError("Typst HTML output did not contain a <body> element")
    return match.group(1)


def strip_duplicate_title(body: str, title: str) -> str:
    match = re.match(r"\s*<h[1-6][^>]*>(.*?)</h[1-6]>\s*", body, flags=re.DOTALL | re.IGNORECASE)
    if not match:
        return body
    heading_text = re.sub(r"<[^>]+>", "", match.group(1))
    heading_text = html_lib.unescape(heading_text)
    if normalize_text(heading_text) == normalize_text(title):
        return body[match.end() :].lstrip()
    return body


def strip_post_metadata(source: str) -> str:
    output: list[str] = []
    skipping = False
    for line in source.splitlines():
        stripped = line.lstrip()
        if not skipping and stripped.startswith("#metadata("):
            skipping = True
            if POST_META_SELECTOR in line:
                skipping = False
            continue
        if skipping:
            if POST_META_SELECTOR in line:
                skipping = False
            continue
        output.append(line)
    return "\n".join(output).lstrip() + "\n"


def run(
    args: list[str],
    *,
    cwd: Path,
    action: str,
    input_text: str | None = None,
) -> subprocess.CompletedProcess[str]:
    try:
        completed = subprocess.run(
            args,
            cwd=cwd,
            text=True,
            input=input_text,
            capture_output=True,
            check=False,
        )
    except FileNotFoundError as exc:
        raise BuildError(f"Required command not found while trying to {action}: {args[0]}") from exc

    if completed.returncode != 0:
        raise BuildError(
            f"Failed to {action}\n"
            f"Command: {' '.join(args)}\n"
            f"stdout:\n{completed.stdout}\n"
            f"stderr:\n{completed.stderr}"
        )
    return completed


def clean_dir(path: Path) -> None:
    if path.exists():
        shutil.rmtree(path)
    path.mkdir(parents=True, exist_ok=True)


def copy_tree_if_exists(source: Path, destination: Path) -> None:
    if not source.exists():
        return
    if destination.exists():
        shutil.rmtree(destination)
    shutil.copytree(source, destination)


def write_text(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def require_str(data: dict[str, object], key: str, path: Path) -> str:
    value = data.get(key)
    if not isinstance(value, str) or not value.strip():
        raise BuildError(f"{path.relative_to(ROOT)} must define a non-empty string field {key!r}")
    return value.strip()


def read_tags(meta: dict[str, object], path: Path) -> list[str]:
    tags = meta.get("tags", [])
    if not isinstance(tags, list) or not all(isinstance(tag, str) and tag.strip() for tag in tags):
        raise BuildError(f"{path.relative_to(ROOT)} tags must be an array of non-empty strings")
    return [tag.strip() for tag in tags]


def parse_date(value: str, path: Path) -> dt.date:
    try:
        return dt.date.fromisoformat(value)
    except ValueError as exc:
        raise BuildError(f"{path.relative_to(ROOT)} date must use YYYY-MM-DD: {value!r}") from exc


def absolute_url(site: SiteConfig, path: str) -> str:
    path = path.strip("/")
    if not site.base_url:
        return ""
    return f"{site.base_url}/{path}" if path else site.base_url + "/"


def format_date(value: dt.date) -> str:
    return f"{value.year}年{value.month}月{value.day}日"


def rfc2822(value: dt.datetime) -> str:
    return value.strftime("%a, %d %b %Y %H:%M:%S %z")


def normalize_text(value: str) -> str:
    return re.sub(r"\s+", " ", value).strip()


def escape(value: str) -> str:
    return html_lib.escape(value, quote=False)


def escape_attr(value: str) -> str:
    return html_lib.escape(value, quote=True)


def url_escape(value: str) -> str:
    return html_lib.escape(value, quote=True)


def yaml_quote(value: str) -> str:
    return value.replace("\\", "\\\\").replace('"', '\\"')


def display_path(path: Path) -> str:
    try:
        return str(path.relative_to(ROOT))
    except ValueError:
        return str(path)


class BuildError(RuntimeError):
    pass


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except BuildError as exc:
        print(f"error: {exc}", file=sys.stderr)
        raise SystemExit(1)
