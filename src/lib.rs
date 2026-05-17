#![allow(unused)]

use anyhow::{Context, Result, bail, ensure};
use chrono::NaiveDate;
use chrono::Utc;
use quick_xml::Writer;
use quick_xml::events::BytesText;
use quick_xml::name;
use regex::Regex;
use serde::Deserialize;
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;
use tempfile::tempdir;

#[derive(Debug)]
pub struct Config {
    pub check: bool,
    pub site_dir: PathBuf,
    pub dist_dir: PathBuf,
}

impl Config {
    pub fn new(arguments: env::Args) -> Result<Config, String> {
        let mut args = arguments.skip(1);

        let mut check = false;
        let mut site_dir = PathBuf::from("_site");
        let mut dist_dir = PathBuf::from("dist");

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--check" | "-c" => {
                    check = true;
                }
                "--site-dir" | "-s" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "--site-dir requires a path".to_string())?;
                    site_dir = PathBuf::from(value);
                }
                "--dist-dir" | "-d" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "--dist-dir requires a path".to_string())?;
                    dist_dir = PathBuf::from(value);
                }
                other => {
                    return Err(format!("unknown argument: {other}"));
                }
            }
        }
        Ok(Config {
            check,
            site_dir,
            dist_dir,
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct SiteConfigFile {
    title: String,
    description: String,
    author: String,
    language: String,
    base_url: String,
}

impl SiteConfigFile {
    fn new(path: PathBuf) -> Result<SiteConfigFile> {
        let content = std::fs::read_to_string(path).context("没有找到 site.toml")?;
        let file_config: SiteConfigFile = toml::from_str(&content).context("解析site.toml失败")?;
        let url = std::env::var("SITE_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(file_config.base_url)
            .trim_end_matches('/')
            .to_string();
        Ok(SiteConfigFile {
            title: file_config.title,
            description: file_config.description,
            author: file_config.author,
            language: file_config.language,
            base_url: url,
        })
    }
}

#[derive(Debug)]
struct Post {
    source: std::path::PathBuf,
    slug: String,
    title: String,
    date: NaiveDate,
    description: String,
    tags: Vec<String>,
    draft: bool,
}

impl Post {
    fn new(source: PathBuf) -> Result<Post> {
        let output = Command::new("typst")
            .args([
                "query",
                "--features",
                "html",
                "--target",
                "html",
                "--root",
                ".",
                source.to_str().unwrap(),
                "<post-meta>",
                "--one",
            ])
            .current_dir(".")
            .output()?;
        ensure!(output.status.success(), "typst query failed");
        let stdout = String::from_utf8(output.stdout)?;
        let value: serde_json::Value = serde_json::from_str(&stdout)?;
        let meta = value.get("value").context("JSON 中没有 value 字段")?;

        let slug = meta
            .get("slug")
            .and_then(|v| v.as_str())
            .context("metadata 中没有有效的 slug 字段")?;

        let title = meta
            .get("title")
            .and_then(|v| v.as_str())
            .context("metadata 中没有有效的 title 字段")?;

        let date = meta
            .get("date")
            .and_then(|v| v.as_str())
            .context("metadata 中没有有效的 date 字段")?;

        let description = meta
            .get("description")
            .and_then(|v| v.as_str())
            .context("metadata 中没有有效的 description 字段")?;

        let tags: Vec<String> = meta
            .get("tags")
            .and_then(|v| v.as_array())
            .context("metadata 中没有有效的 tags 字段")?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(|s| s.to_string())
                    .context("tags 中存在非字符串项")
            })
            .collect::<Result<Vec<String>>>()?;

        let draft = meta
            .get("draft")
            .and_then(|v| v.as_bool())
            .context("metadata 中没有有效的 draft 字段")?;

        Ok(Post {
            source,
            slug: slug.to_string(),
            title: title.to_string(),
            date: NaiveDate::from_str(date)?,
            description: description.to_string(),
            tags,
            draft,
        })
    }
}

fn discover() -> Result<Vec<Post>> {
    let mut posts: Vec<Post> = Vec::new();
    for entry in fs::read_dir("posts").context("不存在posts文件夹")? {
        let entry = entry.context("读取 posts 目录中的文件项失败")?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("typ") {
            // post_paths.push(path);
            posts.push(Post::new(path)?);
        }
    }
    Ok(posts)
}

fn run_command(program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("执行命令失败: {}", program))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("命令执行失败: {}\n{}", program, stderr);
    }
    let stdout = String::from_utf8(output.stdout).context("命令输出不是合法 UTF-8")?;
    Ok(stdout)
}

fn clean_dir(path: &PathBuf) -> Result<()> {
    if path.exists() {
        fs::remove_dir_all(path.as_path())?;
    }
    fs::create_dir_all(path)?;
    Ok(())
}

fn copy_tree_if_exists(src: PathBuf, dst: PathBuf) -> Result<()> {
    if !src.exists() {
        bail!("源目录不存在: {}", src.display());
    }
    if dst.exists() {
        fs::remove_dir_all(&dst).context(format!("删除目标目录失败: {}", &dst.display()))?; // 删除整个目标目录
    }
    fn copy_dir(src: PathBuf, dst: PathBuf) -> Result<()> {
        fs::create_dir_all(&dst)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());
            if file_type.is_dir() {
                copy_dir(src_path, dst_path)?;
            } else {
                fs::copy(&src_path, &dst_path)?;
            }
        }
        Ok(())
    }
    copy_dir(src, dst)?;
    Ok(())
}

fn extract_body(document: &str) -> Result<String> {
    let re = Regex::new(r"(?is)<body>\s*(.*?)\s*</body>").unwrap();
    let body = re
        .captures(document)
        .ok_or_else(|| anyhow::anyhow!("missing body"))?;
    Ok(body[1].to_string())
}

fn strip_duplicate_title(body: &str, title: &str) -> Result<String> {
    let heading_re = Regex::new(r"(?is)^\s*<h[1-6][^>]*>(.*?)</h[1-6]>\s*")?;
    let tag_re = Regex::new(r"(?is)<[^>]+>")?;

    let Some(caps) = heading_re.captures(body) else {
        return Ok(body.to_string());
    };

    let whole_match = caps.get(0).unwrap();
    let heading_html = caps.get(1).map(|m| m.as_str()).unwrap_or("");

    let heading_text = tag_re.replace_all(heading_html, "");
    let heading_text = unescape_html(&heading_text);

    let normalized_heading = heading_text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    let normalized_title = title.split_whitespace().collect::<Vec<_>>().join(" ");

    if normalized_heading == normalized_title {
        Ok(body[whole_match.end()..].trim_start().to_string())
    } else {
        Ok(body.to_string())
    }
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

fn unescape_html(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

fn render_typst_html(post: &Post) -> Result<String> {
    let tmpdir = tempdir()?;
    let path = tmpdir.path().join(format!("{}.html", post.slug));
    let output = run_command(
        "typst",
        &[
            "compile",
            "--features",
            "html",
            "--format",
            "html",
            "--root",
            ".",
            post.source.to_str().unwrap(),
            path.to_str().unwrap(),
        ],
    )?;
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("读取文件失败: {}", path.display()))?;
    let body = extract_body(&content).context("解析失败")?;
    strip_duplicate_title(&body, &post.title)
}

fn render_post_page(site: &SiteConfigFile, post: &Post, article_html: String) -> Result<String> {
    let tags: String = post
        .tags
        .iter()
        .map(|tag| format!("<span>#{}</span>", escape_html(tag)))
        .collect::<Vec<_>>()
        .join(" ");

    let body = format!(
        r#"
<main>
  <article>
    <header>
      <h1 class="post-title">{}</h1>
      <div class="post-meta">
        <time datetime="{}">{}</time>
        {}
      </div>
    </header>
    <div class="post-body">
      {}
    </div>
    <nav class="post-actions" aria-label="文章操作">
      <a href="../../">返回首页</a>
      <a href="../../downloads/{}.md">Markdown 版本</a>
    </nav>
  </article>
</main>
"#,
        escape_html(&post.title),
        (&post.date),
        format_date(post.date),
        tags,
        article_html,
        (&post.slug),
    );
    let cannoical_path = format!("posts/{}", post.slug);
    render_shell(
        site,
        &format!("{} | {}", post.title, site.title),
        &post.description,
        2,
        &body,
        Some(&cannoical_path),
    )
}

fn format_date(date: NaiveDate) -> String {
    date.format("%Y年%-m月%-d日").to_string()
}

fn render_shell(
    site: &SiteConfigFile,
    title: &str,
    description: &str,
    depth: usize,
    body: &str,
    canonical_path: Option<&str>,
) -> Result<String> {
    let prefix = "../".repeat(depth);
    let home_href = if prefix.is_empty() { "./" } else { &prefix };
    let canonical_path = canonical_path.unwrap_or("");
    let canonical = if canonical_path.is_empty() {
        site.base_url.clone()
    } else {
        absolute_url(site, canonical_path)
    };
    let canonical_tag = if canonical.is_empty() {
        String::new()
    } else {
        format!(
            r#"<link rel="canonical" href="{}">"#,
            escape_html(&canonical)
        )
    };
    let html = format!(
        r#"
        <!doctype html>
        <html lang="{}">
        <head>
          <meta charset="utf-8">
          <meta name="viewport" content="width=device-width, initial-scale=1">
          <title>{}</title>
          <meta name="description" content="{}">
          {}
          <link rel="alternate" type="application/rss+xml" title="{}" href="{}feed.xml">
          <link rel="stylesheet" href="{}assets/style.css">
        </head>
        <body>
          <header class="site-header">
            <p class="site-title"><a href="{}">{}</a></p>
            <p class="site-description">{}</p> </header>
        {}
          <footer class="site-footer">
            <span>{}</span>
          </footer>
        </body>
        </html>
"#,
        escape_html(&site.language),
        escape_html(title),
        escape_html(description),
        canonical_tag,
        escape_html(&site.title),
        prefix,
        prefix,
        home_href,
        escape_html(&site.title),
        escape_html(&site.description),
        body.trim_end(),
        escape_html(&site.author)
    )
    .trim_start()
    .to_string();
    Ok(html)
}

fn absolute_url(site: &SiteConfigFile, path: &str) -> String {
    let path = path.trim_matches('/');
    let base = site.base_url.as_str();
    if base.is_empty() {
        return String::new();
    }
    if path.is_empty() {
        format!("{base}/")
    } else {
        format!("{base}/{path}")
    }
}

fn strip_post_metadata(source: &str) -> Result<String> {
    let mut output = Vec::new();
    let mut skip = false;
    for line in source.lines() {
        let stripped = line.trim_start();
        if !skip && stripped.starts_with("#metadata(") {
            skip = true;
            if line.contains("<post-meta>") {
                skip = false;
            }
            continue;
        }
        if skip {
            if line.contains("<post-meta>") {
                skip = false;
            }
            continue;
        }
        output.push(line);
    }
    let joined = output.join("\n");
    Ok(format!("{}\n", joined.trim_start()))
}
fn render_markdown_with_pandoc(post: &Post, source: &str) -> Result<Option<String>> {
    let tmpdir = tempdir()?;
    let path = tmpdir.path().join(format!("{}.md", post.slug));
    let typ_path = tmpdir.path().join("tmp.typ");
    fs::File::create(&typ_path)?.write_all(source.as_bytes());
    run_command(
        "pandoc",
        &[
            "--from",
            "typst",
            "--to",
            "gfm",
            "--wrap",
            "none",
            "--resource-path",
            ".",
            typ_path.to_str().unwrap(),
            "-",
            "--output",
            path.to_str().unwrap(),
        ],
    )?;
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("读取文件失败: {}", path.display()))?;
    Ok(Some(content))
}

fn yaml_quote(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn render_markdown(post: &Post, site: &SiteConfigFile) -> Result<String> {
    let source = strip_post_metadata(&fs::read_to_string(&post.source)?)?;
    let body = render_markdown_with_pandoc(post, &source)?.unwrap();
    let source_url = absolute_url(site, &format!("posts/{}/", post.slug));
    let mut frontmatter = vec![
        "---".to_string(),
        format!("title: \"{}\"", yaml_quote(&post.title)),
        format!("date: \"{}\"", post.date),
        "tags:".to_string(),
    ];
    if !post.tags.is_empty() {
        frontmatter.extend(
            post.tags
                .iter()
                .map(|tag| format!("  - \"{}\"", yaml_quote(tag))),
        );
    } else {
        frontmatter.push(" []".to_string());
    }
    if !source_url.is_empty() {
        frontmatter.push(format!("source: \"{}\"", yaml_quote(&source_url)));
    }
    frontmatter.push("---".to_string());
    Ok(frontmatter.join("\n") + "\n\n" + body.trim() + "\n")
}

fn render_index_page(site: &SiteConfigFile, posts: &Vec<&Post>) -> Result<String> {
    let mut items = posts
        .iter()
        .map(|post| {
            format!(
                r#"<li>
  <h2><a href="posts/{}/">{}</a></h2>
  <time datetime="{}">{}</time>
  <p>{}</p>
</li>"#,
                escape_html(&post.slug),
                escape_html(&post.title),
                post.date.format("%Y-%m-%d"),
                format_date(post.date),
                escape_html(&post.description),
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    if items.is_empty() {
        items = "<li>暂无文章。</li>".to_string();
    }

    let body = format!(
        r#"<main>
  <h1>{}</h1>
  <p>{}</p>
  <ul class="post-list">
    {}
  </ul>
</main>"#,
        escape_html(&site.title),
        escape_html(&site.description),
        items,
    );
    render_shell(site, &site.title, &site.description, 0, &body, None)
}

fn render_robots(site: &SiteConfigFile) -> String {
    let mut robot = vec!["User-agent: *".to_string(), "Allow: /".to_string()];
    if !site.base_url.is_empty() {
        robot.push(format!("Sitemap: {}/sitemap.xml", site.base_url).to_string());
    }
    robot.join("\n") + "\n"
}

fn render_feed(site: &SiteConfigFile, posts: &Vec<&Post>) -> Result<String> {
    let mut writer = Writer::new(Vec::new());
    writer
        .create_element("channel")
        .write_inner_content(|writer| {
            text_element(writer, "title", &site.title);
            text_element(writer, "description", &site.description);
            text_element(writer, "link", &site.base_url);
            text_element(writer, "language", &site.language);
            let latest = posts
                .first()
                .map(|post| post.date)
                .unwrap_or_else(|| Utc::now().date_naive());
            let last_build_date = latest.and_hms_opt(0, 0, 0).unwrap().and_utc().to_rfc2822();
            Ok(())
        })?;
    Ok(String::new())
}

fn text_element<W: Write>(writer: &mut Writer<W>, name: &str, text: &str) -> quick_xml::Result<()> {
    writer
        .create_element(name)
        .write_text_content(BytesText::new(text))?;
    Ok(())
}

fn build(siteconfig: SiteConfigFile, site_path: PathBuf, dist_path: PathBuf) -> Result<()> {
    let posts = discover().context("discover运行失败")?;
    let mut published_posts: Vec<&Post> = posts.iter().filter(|t| !t.draft).collect();
    published_posts.sort_by(|a, b| a.date.cmp(&b.date));
    clean_dir(&site_path)?;
    clean_dir(&dist_path)?;
    let assets_path = site_path.join("assets");
    copy_tree_if_exists(PathBuf::from("assets"), assets_path)?;
    let markdown_dir = site_path.join("downloads");
    fs::create_dir_all(&markdown_dir)?;
    fs::create_dir_all(&dist_path);
    for post in &published_posts {
        let article_html = render_typst_html(post)?;
        let markdown = render_markdown(post, &siteconfig)?;
        let post_dir = site_path.join("posts").join(post.slug.clone());
        fs::create_dir_all(&post_dir)?;
        fs::File::create(post_dir.join("index.html"))?
            .write_all(render_post_page(&siteconfig, post, article_html)?.as_bytes())?;
        fs::File::create(markdown_dir.join(format!("{}.md", post.slug.clone())))?
            .write_all(markdown.as_bytes())?;
        fs::File::create(dist_path.join(format!("{}.md", post.slug.clone())))?
            .write_all(markdown.as_bytes())?;
    }
    fs::File::create(site_path.join("index.html"))?
        .write_all(render_index_page(&siteconfig, &published_posts)?.as_bytes());
    fs::File::create(site_path.join("feed.xml"))?
        .write_all(render_feed(&siteconfig, &published_posts)?.as_bytes());
    fs::File::create(site_path.join("robots.txt"))?
        .write_all(render_robots(&siteconfig).as_bytes())?;
    fs::File::create(site_path.join(".nojekyll"))?;
    Ok(())
}

pub fn run(config: Config) -> Result<()> {
    let siteconfig = SiteConfigFile::new(PathBuf::from("site.toml"))?;
    build(siteconfig, config.site_dir, config.dist_dir)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_file_missing() {
        let path = Path::new("this_file_does_not_exist.toml").to_path_buf();
        let result = SiteConfigFile::new(path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("没有找到"));
    }

    #[test]
    fn test_invalid_toml() -> Result<()> {
        let mut tmp = NamedTempFile::new()?;
        tmp.write_all(b"this is not toml")?;
        let path = tmp.path().to_path_buf();

        let result = SiteConfigFile::new(path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("解析site.toml失败"));

        Ok(())
    }
}
