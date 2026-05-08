# xmoon 的博客

这是一个以 Typst 为主源的静态博客。`posts/*.typ` 是唯一手写正文，HTML 站点和 Markdown 分发版都由构建脚本生成。

## 本地使用

进入固定环境：

```sh
nix develop
```

如果仓库还没有首次提交，Nix 可能提示 `flake.nix` 未被 Git 跟踪；先提交一次，或者临时使用 `nix develop path:$PWD`。

检查完整构建链路：

```sh
python scripts/build.py --check
```

生成站点和 Markdown 分发版：

```sh
python scripts/build.py
```

或者直接使用

```sh
nix develop -c python scripts/build.py
```

输出目录：

- `_site/`：GitHub Pages 发布目录。
- `dist/md/`：Markdown 分发版。
- `_site/downloads/`：站点内可下载的 Markdown 版本。

## 写新文章

在 `posts/` 新增 `.typ` 文件，并在文件开头放置元数据：

```typst
#metadata((
  slug: "lowercase-kebab-case",
  title: "文章标题",
  date: "2026-05-05",
  description: "一句话摘要。",
  tags: ("标签",),
  draft: false,
)) <post-meta>

= 文章标题

正文从这里开始。
```

约定：

- `slug` 只能使用小写 ASCII、数字和短横线，会成为文章 URL：`/posts/<slug>/`。
- `date` 使用 `YYYY-MM-DD`。
- `draft: true` 的文章不会发布。
- Markdown 是生成物，不要手动编辑 `dist/md`。

## 发布到 GitHub Pages

1. 在 GitHub 创建仓库，把默认分支设为 `main`。
2. 推送代码后，在仓库 Settings -> Pages 中选择 GitHub Actions 作为发布源。
3. 可选：在 Settings -> Secrets and variables -> Actions -> Variables 中设置 `SITE_URL`，例如 `https://xmoon.github.io/blog`。设置后 RSS、canonical 和 sitemap 会使用绝对 URL。
4. 每次 push 到 `main` 都会运行 `.github/workflows/pages.yml`，构建 `_site` 并发布到 GitHub Pages。

Typst 的 HTML 导出仍是实验功能，所以这里用 Nix 锁定工具链，并把 Typst 到 HTML、Typst 到 Markdown 的转换都放进 CI 检查。
