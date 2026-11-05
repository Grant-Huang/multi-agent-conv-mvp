@AGENTS.md

# Claude Code 补充说明

- 以上通用规则全部来自 `AGENTS.md`，修改规则请改那个文件。
- 这里只放 Claude Code 特有的说明，目前只有：涉及 `web/` 前端改动时，同时检查 `web/src/bindings/` 下由 Rust 生成的类型是否需要更新（运行 `cargo test -p an-core` 会重新生成）。
