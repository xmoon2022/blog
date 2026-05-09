# Rust 迁移功能库函数教程

对比 `scripts/build.py` 和当前 `src/lib.rs`，你已经做了：

- 命令行参数的基础解析
- `site.toml` 读取
- `SITE_URL` 覆盖 `base_url`
- `typst query` 读取文章 metadata
- 扫描 `posts/*.typ`
- 过滤 draft 的雏形

还没迁移的主要功能，以及 Rust 里对应常用库/函数如下。

---

## 1. 临时目录：对应 Python `tempfile.TemporaryDirectory`

Python 位置：

```py
with tempfile.TemporaryDirectory(prefix="blog-check-") as tmp:
```

Rust 推荐：

```toml
tempfile = "3"
```

示例：

```rust
use tempfile::tempdir;
use std::fs;

fn example() -> anyhow::Result<()> {
    let dir = tempdir()?; // 离开作用域自动删除
    let path = dir.path().join("output.html");

    fs::write(&path, "<html></html>")?;

    println!("临时文件路径: {}", path.display());

    Ok(())
}
```

如果你想控制前缀：

```rust
use tempfile::Builder;

fn example() -> anyhow::Result<()> {
    let dir = Builder::new()
        .prefix("blog-check-")
        .tempdir()?;

    println!("{}", dir.path().display());
    Ok(())
}
```

---

## 2. 清空并重建目录：对应 `clean_dir`

Python：

```py
if path.exists():
    shutil.rmtree(path)
path.mkdir(parents=True, exist_ok=True)
```

Rust：

```rust
use std::fs;
use std::path::Path;

fn clean_dir(path: &Path) -> anyhow::Result<()> {
    if path.exists() {
        fs::remove_dir_all(path)?;
    }

    fs::create_dir_all(path)?;
    Ok(())
}
```

相关函数：

```rust
std::fs::remove_dir_all
std::fs::create_dir_all
Path::exists
```

---

## 3. 递归复制目录：对应 `shutil.copytree`

Python：

```py
shutil.copytree(source, destination)
```

Rust 标准库没有直接的 `copytree`。推荐两种方式。

### 方案 A：使用 `fs_extra`

```toml
fs_extra = "1"
```

示例：

```rust
use fs_extra::dir::{copy, CopyOptions};
use std::path::Path;

fn copy_dir_example(src: &Path, dst: &Path) -> anyhow::Result<()> {
    let mut options = CopyOptions::new();
    options.copy_inside = true;
    options.overwrite = true;

    copy(src, dst, &options)?;

    Ok(())
}
```

### 方案 B：使用 `walkdir` 自己递归

```toml
walkdir = "2"
```

示例：

```rust
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

fn copy_recursive(src: &Path, dst: &Path) -> anyhow::Result<()> {
    for entry in WalkDir::new(src) {
        let entry = entry?;
        let relative = entry.path().strip_prefix(src)?;
        let target = dst.join(relative);

        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), &target)?;
        }
    }

    Ok(())
}
```

---

## 4. 执行外部命令并捕获 stdout/stderr：对应 `subprocess.run`

你已经用了：

```rust
std::process::Command
```

但后面还需要支持：

- 捕获 stdout
- 捕获 stderr
- 传入 stdin，例如 pandoc 的 `input_text`
- 检查退出码
- 命令不存在时报错

示例：

```rust
use std::process::{Command, Stdio};
use std::io::Write;

fn run_command_with_input(cmd: &str, args: &[&str], input: &str) -> anyhow::Result<String> {
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(input.as_bytes())?;
    }

    let output = child.wait_with_output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("command failed: {stderr}");
    }

    let stdout = String::from_utf8(output.stdout)?;
    Ok(stdout)
}
```

相关函数：

```rust
Command::new
Command::args
Command::current_dir
Command::output
Command::spawn
Stdio::piped
Child::wait_with_output
std::io::Write::write_all
```

---

## 5. 检查命令是否存在：对应 `shutil.which("pandoc")`

Python：

```py
if shutil.which("pandoc") is None:
```

Rust 推荐：

```toml
which = "8"
```

示例：

```rust
use which::which;

fn has_pandoc() -> bool {
    which("pandoc").is_ok()
}
```

不用第三方库也可以简单尝试：

```rust
use std::process::Command;

fn command_exists(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .output()
        .is_ok()
}
```

但 `which` 更接近 Python 的 `shutil.which`。

---

## 6. 正则表达式：对应 Python `re`

Python 中用到了：

```py
re.fullmatch(...)
re.match(...)
re.search(..., flags=re.DOTALL | re.IGNORECASE)
re.sub(...)
```

Rust 推荐：

```toml
regex = "1"
```

### slug 校验

Python：

```py
re.fullmatch(r"[a-z0-9]+(?:-[a-z0-9]+)*", slug)
```

Rust：

```rust
use regex::Regex;

fn valid_slug(slug: &str) -> bool {
    let re = Regex::new(r"^[a-z0-9]+(?:-[a-z0-9]+)*$").unwrap();
    re.is_match(slug)
}
```

Rust 的 `regex` 没有单独的 `fullmatch`，通常用 `^...$`。

### 提取 `<body>...</body>`

```rust
use regex::Regex;

fn extract_body(html: &str) -> anyhow::Result<String> {
    let re = Regex::new(r"(?is)<body>\s*(.*?)\s*</body>").unwrap();

    let caps = re
        .captures(html)
        .ok_or_else(|| anyhow::anyhow!("missing body"))?;

    Ok(caps[1].to_string())
}
```

说明：

- `(?i)` = ignore case
- `(?s)` = dot matches newline，也就是 Python 的 `re.DOTALL`

### 替换多空白

Python：

```py
re.sub(r"\s+", " ", value)
```

Rust：

```rust
use regex::Regex;

fn normalize_text(value: &str) -> String {
    let re = Regex::new(r"\s+").unwrap();
    re.replace_all(value, " ").trim().to_string()
}
```

---

## 7. 日期解析和 RFC2822：对应 `datetime.date`

Python：

```py
dt.date.fromisoformat(value)
value.strftime("%a, %d %b %Y %H:%M:%S %z")
```

Rust 推荐：

```toml
chrono = "0.4"
```

### 解析 `YYYY-MM-DD`

```rust
use chrono::NaiveDate;

fn parse_date(value: &str) -> anyhow::Result<NaiveDate> {
    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d")?;
    Ok(date)
}
```

### 输出 ISO 日期

```rust
use chrono::NaiveDate;

fn example(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}
```

### 中文日期格式

Python：

```py
f"{value.year}年{value.month}月{value.day}日"
```

Rust：

```rust
use chrono::{NaiveDate, Datelike};

fn format_date(date: NaiveDate) -> String {
    format!("{}年{}月{}日", date.year(), date.month(), date.day())
}
```

### RSS 的 RFC2822 时间

```rust
use chrono::{NaiveDate, NaiveTime, DateTime, Utc};

fn rfc2822(date: NaiveDate) -> String {
    let datetime = date.and_time(NaiveTime::MIN);
    let utc: DateTime<Utc> = DateTime::from_naive_utc_and_offset(datetime, Utc);

    utc.to_rfc2822()
}
```

---

## 8. HTML 转义：对应 Python `html.escape`

Python：

```py
html_lib.escape(value, quote=False)
html_lib.escape(value, quote=True)
```

Rust 推荐：

```toml
html-escape = "0.2"
```

示例：

```rust
use html_escape::{encode_text, encode_double_quoted_attribute};

fn escape_text(value: &str) -> String {
    encode_text(value).to_string()
}

fn escape_attr(value: &str) -> String {
    encode_double_quoted_attribute(value).to_string()
}
```

区别：

```rust
let text = escape_text("<hello>");
let attr = escape_attr("\"hello\"");

println!("{text}");
println!("{attr}");
```

常见用途：

- HTML 正文内容：`encode_text`
- HTML 属性值：`encode_double_quoted_attribute`

---

## 9. XML 生成：对应 `xml.etree.ElementTree`

Python：

```py
ET.Element("rss", version="2.0")
ET.SubElement(channel, "title").text = site.title
ET.tostring(rss, encoding="unicode")
```

Rust 推荐：

```toml
quick-xml = "0.38"
```

示例：

```rust
use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};

fn xml_example() -> anyhow::Result<String> {
    let mut writer = Writer::new(Vec::new());

    writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("utf-8"), None)))?;

    writer.write_event(Event::Start(BytesStart::new("rss")))?;

    writer.write_event(Event::Start(BytesStart::new("channel")))?;

    writer.write_event(Event::Start(BytesStart::new("title")))?;
    writer.write_event(Event::Text(BytesText::new("My Blog")))?;
    writer.write_event(Event::End(BytesEnd::new("title")))?;

    writer.write_event(Event::End(BytesEnd::new("channel")))?;
    writer.write_event(Event::End(BytesEnd::new("rss")))?;

    let result = String::from_utf8(writer.into_inner())?;
    Ok(result)
}
```

带属性示例：

```rust
use quick_xml::events::BytesStart;

let mut elem = BytesStart::new("rss");
elem.push_attribute(("version", "2.0"));
```

如果你觉得 `quick-xml` 太底层，也可以考虑：

```toml
xmlwriter = "0.1"
```

它更适合手写简单 XML。

---

## 10. 多行 HTML 模板：对应 `textwrap.dedent(f"""...""")`

Python：

```py
textwrap.dedent(
    f"""
    <main>
      <h1>{escape(site.title)}</h1>
    </main>
    """
)
```

Rust 推荐：

```toml
indoc = "2"
```

示例：

```rust
use indoc::formatdoc;

fn html_page(title: &str, body: &str) -> String {
    formatdoc! {r#"
        <!doctype html>
        <html>
        <head>
          <title>{title}</title>
        </head>
        <body>
          {body}
        </body>
        </html>
    "#}
}
```

如果不想加依赖，也可以直接用 `format!`：

```rust
fn html_page(title: &str) -> String {
    format!(
        r#"<!doctype html>
<html>
<head>
  <title>{}</title>
</head>
<body>
</body>
</html>
"#,
        title
    )
}
```

---

## 11. 写文本文件并自动创建父目录：对应 `write_text`

Python：

```py
path.parent.mkdir(parents=True, exist_ok=True)
path.write_text(content, encoding="utf-8")
```

Rust：

```rust
use std::fs;
use std::path::Path;

fn write_text(path: &Path, content: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(path, content)?;
    Ok(())
}
```

相关函数：

```rust
Path::parent
std::fs::create_dir_all
std::fs::write
```

---

## 12. 读取文本文件：对应 `Path.read_text`

Python：

```py
path.read_text(encoding="utf-8")
```

Rust：

```rust
use std::fs;
use std::path::Path;

fn read_text(path: &Path) -> anyhow::Result<String> {
    let content = fs::read_to_string(path)?;
    Ok(content)
}
```

相关函数：

```rust
std::fs::read_to_string
```

---

## 13. 路径处理：对应 Python `Path`

常用映射：

| Python | Rust |
|---|---|
| `ROOT / "posts"` | `root.join("posts")` |
| `path.exists()` | `path.exists()` |
| `path.parent` | `path.parent()` |
| `path.relative_to(ROOT)` | `path.strip_prefix(root)` |
| `path.suffix` | `path.extension()` |
| `path.name` | `path.file_name()` |

示例：

```rust
use std::path::{Path, PathBuf};

fn path_example() -> anyhow::Result<()> {
    let root = PathBuf::from(".");
    let posts = root.join("posts");

    let file = posts.join("001.typ");

    if file.exists() {
        println!("exists");
    }

    if file.extension().and_then(|x| x.to_str()) == Some("typ") {
        println!("typst file");
    }

    let relative = file.strip_prefix(&root)?;
    println!("{}", relative.display());

    Ok(())
}
```

---

## 14. 扫描 `posts/*.typ` 并排序：对应 `Path.glob("*.typ")`

你现在用了 `fs::read_dir`，这是正确方向。需要注意 Python 版本有排序：

```py
post_files = sorted(POSTS_DIR.glob("*.typ"))
```

Rust 示例：

```rust
use std::fs;
use std::path::PathBuf;

fn typ_files() -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for entry in fs::read_dir("posts")? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("typ") {
            files.push(path);
        }
    }

    files.sort();

    Ok(files)
}
```

如果想用 glob 风格：

```toml
glob = "0.3"
```

```rust
use glob::glob;

fn glob_example() -> anyhow::Result<Vec<std::path::PathBuf>> {
    let mut files = Vec::new();

    for entry in glob("posts/*.typ")? {
        files.push(entry?);
    }

    files.sort();
    Ok(files)
}
```

---

## 15. 检查重复 slug：对应 Python `dict`

Python：

```py
slugs: dict[str, Path] = {}
if post.slug in slugs:
    raise BuildError(...)
```

Rust：

```rust
use std::collections::HashMap;
use std::path::PathBuf;

fn check_duplicate_slug(posts: Vec<(String, PathBuf)>) -> anyhow::Result<()> {
    let mut slugs: HashMap<String, PathBuf> = HashMap::new();

    for (slug, path) in posts {
        if let Some(previous) = slugs.insert(slug.clone(), path.clone()) {
            anyhow::bail!(
                "duplicate slug {slug}: {} and {}",
                previous.display(),
                path.display()
            );
        }
    }

    Ok(())
}
```

如果只关心是否重复，也可以用 `HashSet`：

```rust
use std::collections::HashSet;

let mut seen = HashSet::new();

if !seen.insert("abc".to_string()) {
    println!("duplicate");
}
```

---

## 16. 按日期和 slug 倒序排序

Python：

```py
published_posts.sort(key=lambda post: (post.date, post.slug), reverse=True)
```

Rust 示例：

```rust
posts.sort_by(|a, b| {
    b.date
        .cmp(&a.date)
        .then_with(|| b.slug.cmp(&a.slug))
});
```

如果 `date` 是 `String` 且格式固定为 `YYYY-MM-DD`，字符串排序可用；但更稳妥是用：

```rust
chrono::NaiveDate
```

---

## 17. 解析 JSON metadata：建议用 typed struct 替代 `serde_json::Value`

你现在手动：

```rust
let value: serde_json::Value = serde_json::from_str(&stdout)?;
let meta = value.get("value")...
```

可以继续这样做；但 Rust 里更推荐结构化反序列化。

示例：

```rust
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct QueryOutput {
    value: PostMeta,
}

#[derive(Debug, Deserialize)]
struct PostMeta {
    slug: String,
    title: String,
    date: String,
    description: String,
    tags: Vec<String>,

    #[serde(default)]
    draft: bool,
}

fn parse_metadata(json: &str) -> anyhow::Result<PostMeta> {
    let output: QueryOutput = serde_json::from_str(json)?;
    Ok(output.value)
}
```

好处：

- 字段类型错误会自动报错
- `draft` 可用 `#[serde(default)]` 对应 Python 的 `meta.get("draft", False)`
- 少写很多 `get(...).and_then(...)`

---

## 18. 移除 Typst metadata 块：对应 `strip_post_metadata`

Python 逻辑是逐行扫描：

```py
for line in source.splitlines():
    ...
```

Rust 示例：

```rust
fn strip_metadata(source: &str) -> String {
    let mut output = Vec::new();
    let mut skipping = false;

    for line in source.lines() {
        let stripped = line.trim_start();

        if !skipping && stripped.starts_with("#metadata(") {
            skipping = true;

            if line.contains("<post-meta>") {
                skipping = false;
            }

            continue;
        }

        if skipping {
            if line.contains("<post-meta>") {
                skipping = false;
            }

            continue;
        }

        output.push(line);
    }

    format!("{}\n", output.join("\n").trim_start())
}
```

相关函数：

```rust
str::lines
str::trim_start
str::starts_with
str::contains
Vec::push
Vec::join
```

---

## 19. Markdown fallback：对应简单标题转换

Python：

```py
match = re.match(r"^(=+)\s+(.*)$", line)
```

Rust：

```rust
use regex::Regex;

fn markdown_fallback(source: &str) -> String {
    let heading = Regex::new(r"^(=+)\s+(.*)$").unwrap();
    let mut converted = Vec::new();

    for line in source.lines() {
        if let Some(caps) = heading.captures(line) {
            let level = caps[1].len();
            let title = &caps[2];
            converted.push(format!("{} {}", "#".repeat(level), title));
        } else {
            converted.push(line.to_string());
        }
    }

    format!("{}\n", converted.join("\n").trim())
}
```

---

## 20. YAML 字符串转义：对应 `yaml_quote`

Python：

```py
value.replace("\\", "\\\\").replace('"', '\\"')
```

Rust：

```rust
fn yaml_quote(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}
```

---

## 21. 绝对 URL 拼接：对应 `absolute_url`

Python：

```py
path = path.strip("/")
if not site.base_url:
    return ""
return f"{site.base_url}/{path}" if path else site.base_url + "/"
```

Rust：

```rust
fn absolute_url(base_url: &str, path: &str) -> String {
    let path = path.trim_matches('/');

    if base_url.is_empty() {
        return String::new();
    }

    if path.is_empty() {
        format!("{base_url}/")
    } else {
        format!("{base_url}/{path}")
    }
}
```

如果你以后要处理复杂 URL，推荐：

```toml
url = "2"
```

不过当前 Python 逻辑只是简单字符串拼接，用标准库就够。

---

## 22. 去掉 Typst HTML 里重复标题：对应 `strip_duplicate_title`

涉及三步：

1. 正则找开头第一个 `<h1>` 到 `<h6>`
2. 去掉 HTML 标签
3. HTML entity unescape
4. 归一化空白后比较

推荐依赖：

```toml
regex = "1"
html-escape = "0.2"
```

示例：

```rust
use regex::Regex;
use html_escape::decode_html_entities;

fn remove_html_tags(value: &str) -> String {
    let re = Regex::new(r"<[^>]+>").unwrap();
    re.replace_all(value, "").to_string()
}

fn normalize(value: &str) -> String {
    let re = Regex::new(r"\s+").unwrap();
    re.replace_all(value, " ").trim().to_string()
}

fn example(body: &str, title: &str) -> String {
    let heading = Regex::new(r"(?is)^\s*<h[1-6][^>]*>(.*?)</h[1-6]>\s*").unwrap();

    if let Some(caps) = heading.captures(body) {
        let heading_html = &caps[1];
        let plain = remove_html_tags(heading_html);
        let unescaped = decode_html_entities(&plain);

        if normalize(&unescaped) == normalize(title) {
            let matched = caps.get(0).unwrap();
            return body[matched.end()..].trim_start().to_string();
        }
    }

    body.to_string()
}
```

---

## 23. 生成 Markdown frontmatter

Python 是拼字符串。Rust 也可以直接拼。

示例：

```rust
fn frontmatter(title: &str, date: &str, tags: &[String]) -> String {
    let mut lines = Vec::new();

    lines.push("---".to_string());
    lines.push(format!("title: \"{}\"", yaml_quote(title)));
    lines.push(format!("date: \"{}\"", date));
    lines.push("tags:".to_string());

    if tags.is_empty() {
        lines.push("  []".to_string());
    } else {
        for tag in tags {
            lines.push(format!("  - \"{}\"", yaml_quote(tag)));
        }
    }

    lines.push("---".to_string());

    lines.join("\n")
}

fn yaml_quote(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}
```

---

## 24. 推荐新增依赖清单

按你当前还缺的功能，最实用的是这些：

```toml
regex = "1"
chrono = "0.4"
html-escape = "0.2"
quick-xml = "0.38"
indoc = "2"
which = "8"
walkdir = "2"
fs_extra = "1"
```

其中：

- `regex`：slug 校验、HTML body 提取、标题处理、Markdown fallback
- `chrono`：日期解析、RSS 时间
- `html-escape`：HTML/XML 文本转义、反转义
- `quick-xml`：RSS、sitemap
- `indoc`：多行 HTML 模板
- `which`：检测 pandoc 是否存在
- `walkdir` / `fs_extra`：递归复制 assets/public

`walkdir` 和 `fs_extra` 二选一即可。
如果你想少写代码，用 `fs_extra`。如果你想完全控制复制行为，用 `walkdir`。
