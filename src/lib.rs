use anyhow::{Context, Result, bail, ensure};
use core::error;
use core::slice;
use serde::Deserialize;
use serde::de;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

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
    fn new(config: Config) -> Result<SiteConfigFile, Box<dyn error::Error>> {
        let content = std::fs::read_to_string("site.toml")?;
        let file_config: SiteConfigFile = toml::from_str(&content)?;
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

pub fn run(config: Config) -> Result<(), Box<dyn error::Error>> {
    let siteconfig = SiteConfigFile::new(config);
    println!("{:#?}", siteconfig);
    let mut posts: Vec<PathBuf> = Vec::new();
    for entry in fs::read_dir("posts")? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("typ") {
            posts.push(path);
        }
    }
    println!("{:#?}", posts);
    let output = Command::new("typst")
        .args([
            "query",
            "--features",
            "html",
            "--target",
            "html",
            "--root",
            ".",
            "posts/001-为什么决定写博客.typ",
            "<post-meta>",
            "--one",
        ])
        .current_dir(".")
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("typst query failed: {stderr}").into());
    }

    let stdout = String::from_utf8(output.stdout)?;
    let value: serde_json::Value = serde_json::from_str(&stdout)?;
    println!("{}", serde_json::to_string_pretty(&value)?);

    let meta = value.get("value").context("JSON 中没有 value 字段")?;

    let slug = meta
        .get("slug")
        .and_then(|v| v.as_str())
        .context("metadata 中没有有效的 slug 字段")?;

    let title = meta
        .get("title")
        .and_then(|v| v.as_str())
        .context("metadata 中没有有效的 title 字段")?;

    println!("slug = {}", slug);
    println!("title = {}", title);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_post() -> anyhow::Result<()> {
        let posts = discover()?;
        for post in posts {
            dbg!(post);
        }
        Ok(())
    }
}
