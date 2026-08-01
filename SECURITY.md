# 安全政策

## 支持范围

安全更新优先覆盖 GitHub Releases 中的最新稳定版本。旧版本用户应先升级到最新版本再复现问题。

## 报告漏洞

请使用 GitHub 仓库的 **Security → Report a vulnerability** 私密报告功能。不要在公开 Issue、讨论、截图或日志中提交以下内容：

- Yuxi API Key、JWT、Cookie 或其他凭证；
- 未公开漏洞的完整利用步骤；
- 包含真实用户、会话或研究数据的数据库与日志。

报告中请提供受影响版本、操作系统、最小复现步骤、预期影响和建议修复方式。维护者会尽快确认并协调修复与披露。

## 客户端安全模型

- API Key 位于 Stronghold 加密 vault，vault 解锁材料由操作系统凭据保险库保存。
- 远程网关只接受 HTTPS；HTTP 仅用于回环地址开发。
- 应用不加载远程网页脚本，不在 WebView 中直接向 Yuxi 发起带凭证请求。
- GitHub 自动更新产物必须通过仓库配置的 Tauri 私钥签名；私钥不得提交到 Git。

如果怀疑 Key 已泄露，请立即在 Yuxi 管理后台禁用/删除该 Key 并生成新的最小权限 Key。
