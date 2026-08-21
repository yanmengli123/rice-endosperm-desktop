# 更新日志

## 0.1.4（2026-08-21）

- 修复 Tauri Channel 最终事件与命令返回值之间的时序竞争，始终以 Yuxi 服务端返回的权威 `output` 更新最终回答。
- 流式预览按 Yuxi `message_id` 隔离多轮模型消息，避免把知识检索规划、工具调用过程和最终回答拼接成乱码。
- 桌面端只渲染 `message_delta.content`，不展示 `reasoning_content`、工具参数或子线程内容。
- Yuxi 已完成但最终消息仍在落库时会短暂重试，不再退回内部流式文本作为最终答案。

## 0.1.3（2026-08-21）

- 适配最新版 `rice-endosperm-agent` 的思考模型多轮工具调用协议，修复 DeepSeek 等模型因缺少 `reasoning_content` 回传而导致的 400 错误。
- 增加旧版 Yuxi 服务端兼容性检测：异常响应不会再作为正常回答写入本地聊天记录，并会给出可执行的服务端升级提示。
- 服务端模型兼容性修复已同步至 `yanmengli123/rice-endosperm-agent` 的 `main` 分支。

## 0.1.2

- 完善 Windows NSIS/MSI 安装包与 Tauri 签名自动更新流程。
- 改进客户端连接、会话恢复与本地安全存储。
