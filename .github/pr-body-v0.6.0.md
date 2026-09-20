## 概述

本 PR 把桌面端接入服务端 AgentRun 1.4 的 **additive 证据平面**（对应服务端 `feat/pdf-evidence-v1`）。协议 major 仍为 1.4，无破坏性变更。

## 变更内容

### 协议底座（PR-1）
- 连接成功后将 `GET /api/agent/protocol` 能力快照缓存进 AppState；major 不一致 fail-fast 为 `protocol_incompatible`（与服务端 426 同码）。
- SSE custom 二级白名单：`payload.name` 未知记 WARN `sse_custom_unknown`，避免证据类事件被静默丢弃。
- 澄清问题全栈：interrupt + ClarificationCard + `{questionId: 答案}` resume（与 Web buildAnswer 同构，`__other__` 三端同源）。
- 契约语料同步副本 + cargo 回放 + manifest sha256 校验 + 白名单↔语料联动断言。

### 证据平面（PR-2）
- `citation_ready` v2（九键 citation + 可选 figures）、`locator_candidates`、`yuxi.warning` 进 `EvidencePlane`。
- 图卡取图：四键齐备才构造资产 URL，经 `get_figure_asset` 拉 base64 data URL（字节不落盘）；缺键 fail-closed；受 `figures_card` 能力位门控。
- 历史恢复：证据落库 `messages.evidence_json`（迁移 0008），从本地真源恢复，不重放 SSE。

### 服务端配套（同工作区，Yuxi 仓库）
- APISIX 桌面路由放行 `GET /api/knowledge/databases/:kb_id/documents/:file_id/revisions/:revision_id/assets/*`（仅 GET，上游鉴权）。
- `test_apisix_gateway_contract.py` REQUIRED_ROUTES 同步锁定。
- **部署注意**：改 `apisix.yaml` 后必须 force-recreate APISIX 容器才生效。

## 验证
- 本地：`pnpm check` 全绿（TypeScript + Vite + vitest 16 文件 80 测试）。
- CI frontend：绿。
- CI rust：`cargo fmt` 已修复后重跑；clippy/test 在 windows-latest MSVC 执行。
- 本机无 MSVC `link.exe`，rust 链接/测试以 CI 为准。

## 发布
- 版本已对齐 `0.6.0`（package.json / Cargo.toml / tauri.conf.json）。
- 合并 main 且 CI 全绿后打 tag `v0.6.0`，由 `release.yml` 构建 NSIS/MSI + 更新签名。

## 依赖与边界
- 不抢先发送 mention.v2 未知 body 字段（APISIX `/api/agent/runs` 闭集）。
- `figure_card_enabled` 服务端默认关闭；桌面端仅在能力位声明时尝试取图，开闸语义由服务端控制。
