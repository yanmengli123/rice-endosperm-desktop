# 贡献指南

欢迎通过 Issue 和 Pull Request 改进稻芯智析 Desktop。提交前请：

1. 从 `main` 创建短生命周期分支。
2. 不提交任何 API Key、`.env`、Stronghold vault、SQLite 数据库、日志或签名私钥。
3. 保持前端 TypeScript strict 和 Rust `clippy -D warnings` 通过。
4. 为协议解析、安全边界和数据迁移补充测试。
5. 使用 Conventional Commits，例如 `feat: 增加会话导出`、`fix: 修复 SSE 恢复游标`。

提交前运行 README 中的质量检查。UI 变更请在 PR 中附截图，但先确认截图不含真实凭证或敏感研究数据。
