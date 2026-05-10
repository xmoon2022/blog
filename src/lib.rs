use anyhow::{Context, Result, bail, ensure};
use regex::Regex;
use serde::Deserialize;
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
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
    base_url: Option<String>,
}

impl SiteConfigFile {
    fn new(path: PathBuf) -> Result<SiteConfigFile> {
        let content = std::fs::read_to_string(path).context("没有找到 site.toml")?;
        let file_config: SiteConfigFile = toml::from_str(&content).context("解析site.toml失败")?;
        let base_url = std::env::var("SITE_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or(file_config.base_url)
            .unwrap_or_default()
            .trim_end_matches('/')
            .to_string();
        Ok(SiteConfigFile {
            title: file_config.title,
            description: file_config.description,
            author: file_config.author,
            language: file_config.language,
            base_url: Some(base_url),
        })
    }
}

#[derive(Debug)]
struct Post {
    source: std::path::PathBuf,
    slug: String,
    title: String,
    date: String,
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
            date: date.to_string(),
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

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

fn render_typst_html(post: &Post) -> Result<String> {
    let tmpdir = tempdir()?;
    let path = tmpdir.path().join(format!("{}.html", post.slug));
    let outputs = Command::new("typst")
        .args([
            "compile",
            "--features",
            "html",
            "--format",
            "html",
            "--root",
            ".",
            post.source.to_str().unwrap(),
            path.to_str().unwrap(),
        ])
        .current_dir(".")
        .output()
        .context("执行 typst compile 失败")?;

    ensure!(
        outputs.status.success(),
        "typst compile 失败:\n{}",
        String::from_utf8_lossy(&outputs.stderr)
    );

    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("读取文件失败: {}", path.display()))?;
    let body = extract_body(&content).context("解析失败")?;
    Ok(body)
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

    <div class="post-content">
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
        (&post.date),
        escape_html(&tags),
        article_html,
        (&post.slug),
    );
    Ok(body)
}

fn build(siteconfig: SiteConfigFile, site_path: PathBuf, dist_path: PathBuf) -> Result<()> {
    let posts = discover().context("discover运行失败")?;
    let mut published_posts: Vec<&Post> = posts.iter().filter(|t| !t.draft).collect();
    published_posts.sort_by(|a, b| a.date.cmp(&b.date));
    clean_dir(&site_path)?;
    clean_dir(&dist_path)?;
    let mut assets_path = site_path.clone();
    assets_path.push("assets");
    copy_tree_if_exists(PathBuf::from("assets"), assets_path)?;
    for post in published_posts {
        let article_html = render_typst_html(post)?;
        let post_dir = site_path.join("posts").join(post.slug.clone());
        fs::create_dir_all(&post_dir)?;
        fs::File::create(post_dir.join("index.html"))?
            .write_all(render_post_page(&siteconfig, post, article_html)?.as_bytes())?;
    }
    fs::File::create(site_path.join(".nojekyll"))?;
    let mut robot_file = fs::File::create(site_path.join("robots.txt"))?;
    robot_file.write_all(b"User-agent: *\nAllow: /")?;
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
