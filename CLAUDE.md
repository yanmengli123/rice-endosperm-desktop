# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目概述

稻芯智析（daoxin-zhixi-desktop）：面向水稻胚乳科研问答的 Tauri 2 + React 19 Windows 桌面客户端。以 Yuxi 作为 Agent 业务内核，通过用户配置的 Yuxi API Key 连接 APISIX/Yuxi 网关，走异步 Agent run + SSE 增量输出。配套服务端改造位于同工作区的 [../Yuxi](../Yuxi)（github: rice-endosperm-agent）。

## 常用命令

环境要求：Node.js 22+、pnpm 10、Rust stable（工具链锁定 `1.97.1-x86_64-pc-windows-msvc`，见 rust-toolchain.toml）、WebView2。

```powershell
pnpm install --frozen-lockfile
. .\.github\scripts\prepare-libsodium.ps1   # Windows 首次编译 Stronghold 前必须执行：下载 libsodium DLL 并放入打包资源，避免静态 CRT 冲突
pnpm tauri dev                              # 开发模式（Vite :1420 + Tauri 窗口）

pnpm check                                  # 质量检查 = tsc --noEmit + vite build + vitest run
pnpm test                                   # 仅前端 Vitest
pnpm vitest run src/runtime/yuxi-adapter.test.ts                 # 运行单个前端测试文件
cargo fmt     --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy  --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings   # CI 标准：警告即失败
cargo test    --manifest-path src-tauri/Cargo.toml                            # Rust 测试
cargo test    --manifest-path src-tauri/Cargo.toml <测试名>                    # 运行单个 Rust 测试

pnpm tauri build --bundles nsis,msi         # 打包；可用 $env:YUXI_BASE_URL / $env:YUXI_AGENT_SLUG 设置新用户首次默认值（仅默认值，不含 Key）
```

**libsodium 环境变量**：`cargo check/clippy/test` 依赖 `SODIUM_LIB_DIR` 和 `SODIUM_SHARED=1`（否则 libsodium-sys 构建脚本会自行下载并可能失败）。prepare-libsodium.ps1 只在当前 PowerShell 会话设置这两个变量——**新开终端跑 cargo 命令前必须重新 export**（脚本下载的 dist 在 `%TEMP%\daoxin-libsodium-dist\extracted\libsodium\x64\Release\v143\dynamic`）。三个坑：Windows TEMP 清理可能删掉该 dist，表现为 cargo test 链接期 `LNK1181: 无法打开输入文件"libsodium.lib"`（cargo check/clippy 不链接所以能过），重跑 prepare-libsodium.ps1 即恢复——TEMP 清理频繁时建议把 extracted 目录拷到仓库外固定路径（如 `.tooling/`，已 gitignore）并把 `SODIUM_LIB_DIR` 指过去；Git Bash 会话中需手动 `export SODIUM_LIB_DIR='C:\...\dynamic' SODIUM_SHARED=1`；新增 Rust 依赖后 `cargo test` 首次链接同样需要该变量。

发布流程：打 tag `vX.Y.Z` 推送后由 GitHub Actions 构建并发布安装包，更新签名私钥只在 Actions Secrets；完整流程见 [docs/RELEASING.md](docs/RELEASING.md)。

## 架构

两层结构，边界清晰：

**React 前端（src/，TypeScript strict）**

- `App.tsx` 组合 `components/`：ChatWorkspace（assistant-ui 对话主界面）、ConnectionSetup（网关地址/API Key 配置）、Sidebar、SettingsDialog、RunContextBar。
- `runtime/yuxi-adapter.ts` 把 Tauri 事件适配成 assistant-ui 对话流（SSE 增量、`Last-Event-ID` 断线续传、原任务结果恢复）。
- `services/tauri-client.ts` 是唯一的前端 → Rust 调用封装（tauri invoke），组件不要散落直接调 invoke。
- 测试与源码同目录（`.test.tsx` / `.test.ts`），Vitest + Testing Library + jsdom。

**Tauri/Rust 后端（src-tauri/src/）**

- `lib.rs` 注册全部 invoke 命令与插件；新增命令必须在 `generate_handler![]` 中注册。
- `commands.rs` 命令层；`state.rs` 全局 AppState（启动时打开 SQLite 并初始化诊断）。发起网关请求的命令一律先经 `ensure_active_bearer` 取 Bearer 凭证（会话令牌主用 + 临近过期自动轮换 + 过渡 Key 回退），不要直接调 `credentials.api_key()`。
- `yuxi.rs` 与 Yuxi 网关的 HTTP/SSE 客户端（reqwest + eventsource-stream）：创建 run、拉取事件流、取消、恢复结果、旋转会话。
- `credentials.rs` 凭据管理：Stronghold 加密 vault + OS keyring。**多账号存储模型**——`b"yuxi-api-key:{scope}"` / `b"yuxi-session:{scope}"` 是按账号作用域隔离的真源；`b"yuxi-api-key"`（无后缀）只是"当前选中账号"的缓存，切换账号即拷贝作用域记录到 ACTIVE。Key 绝不能进 SQLite、浏览器存储或日志。
- `session.rs` 会话令牌本地结构（access/refresh/family + 过期时间）与 JWT exp 解析；过期判断不做签名校验（服务端负责），仅用于触发轮换。
- `database.rs` + `migrations/`：sqlx SQLite 存会话、消息和运行恢复状态；迁移 v5 起含 `accounts` 账号目录表（切换器数据源）；schema 变更加新迁移文件。
- `config.rs` 编译期默认值（`YUXI_BASE_URL` 等）；`diagnostics.rs` 日志与 panic hook。

## 关键约束

- 远程网关必须 HTTPS，仅本机开发地址允许 http；CSP 在 src-tauri/tauri.conf.json 中严格限定，引入外部资源需同步审查。
- 协议解析（SSE/run 状态机）、安全边界（凭据、HTTPS）和数据迁移的改动必须带测试（CONTRIBUTING.md 要求）。
- 不提交任何 API Key、`.env`、Stronghold vault、SQLite 数据库、日志或签名私钥。
- 提交信息使用 Conventional Commits 中文描述（如 `fix: 修复 SSE 恢复游标`）；UI 变更在 PR 中附截图，注意截图不得含真实凭证或敏感研究数据。

## 与服务端的契约

桌面端依赖 Yuxi 服务端暴露的一组 API Key 认证接口（AgentRun 契约 v1.1）：`/api/agent-invocation/*` 与 `/api/agent/runs/{run_id}/*`；设备码登录走 `/api/auth/cli/sessions*`（token 响应携带服务端签发的不可逆 `account_scope_id` 与可选 `session` 会话对——短时访问令牌 + 旋转刷新令牌 + `session_id`，本地 SQLite/Stronghold 数据按 `account_scope_id` 隔离账号，不落盘原始 uid）；会话续期走 `POST /api/auth/cli/token/refresh`（重放检测由服务端会话族承担）；模型偏好统一存服务端 `GET/PUT /api/user/model-preference`，聊天发起时不带 `model_spec`、由服务端解析。完整清单见 README「使用前提」。修改这些接口时，服务端（../Yuxi）与本仓库必须同步调整。
