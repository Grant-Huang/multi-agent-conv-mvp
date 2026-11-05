---
name: websearch
description: 联网搜索并读取网页正文。需要最新资料、技术文档、新闻、外部事实，或用户要求搜索、联网查、给出处时使用。用法是直接在 bash 里运行 `websearch search-fetch "关键词" -n 3`，一条命令完成搜索并返回前几条网页正文（JSON）；这是已安装的命令行工具，不要写 Python 代码，也不需要导入任何包。
---

# 联网搜索（websearch）

用 bash 执行 `websearch` 命令（需已安装 agent-websearch，且 `websearch` 在 PATH 中）。输出是一行 JSON。

## 首选：搜索并抓取正文（一条命令）

```bash
websearch search-fetch "搜索关键词" -n 3 --max-chars 3000
```

- 关键词用简洁的短语，不要整句提问；中文问题用中文关键词，技术名词保留英文原文。
- 默认搜索是 Exa，不要加 `--provider`。备选是 `--provider ddg` 或 `--provider parallel`。搜索失败不会自动换源。
- 用户问最近、今天或近 24 小时时，把日期写进关键词，例如 `2026-11-05`。
- 返回 JSON 的 `data.pages` 是抓取到的页面正文（Markdown），每项有 `url`、`title`、`content`。
- `data.failures` 是抓取失败的页面；`data.results` 是全部搜索结果（标题、URL、摘要），正文抓不到时可参考摘要。
- 没有结果时换一种说法再搜，连同第一次最多 3 次。不要因为空结果改搜索源。

## 只想先看有哪些来源

```bash
websearch search "关键词" -n 8
```

## 读取指定网页

```bash
websearch fetch "https://example.com/page" --max-chars 8000
```

## 回答要求

- 只依据返回的内容回答；搜不到或内容不相关时直接说明，不要编造。
- 回答末尾列出实际引用的来源链接（`url`）。
- 如果 `status` 是 `error`，读 `message` 说明原因，必要时换关键词重试。
