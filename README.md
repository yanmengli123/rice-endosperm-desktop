# 稻芯智析 Desktop

<img src="public/brand-logo.png" width="180" alt="稻芯智析徽标">

稻芯智析是面向水稻胚乳发育、灌浆调控、基因功能与组学分析的双引擎科研桌面工作台。知识问答以 [Yuxi](https://github.com/xerrors/Yuxi) 作为远程 Agent 业务内核；本地科研计算由独立的 [rice-endosperm-workflow](https://github.com/yanmengli123/rice-endosperm-workflow) WISP Sidecar 执行。

> 当前公开版是自托管客户端：首次启动时填写管理员发放的登录 ID（或用户名）、初始密码、Yuxi API Key 和网关地址。三项凭据会由服务端原子校验，客户端不会自行拼接身份。本地默认地址为 `http://127.0.0.1:9088`；远程网关必须使用 HTTPS。

## 下载与安装

前往 [GitHub Releases](https://github.com/yanmengli123/rice-endosperm-desktop/releases/latest) 下载 Windows 安装包：

- 推荐普通用户下载 `Daoxin-Zhixi_*_x64-setup.exe`（NSIS）。
- 需要企业软件分发时下载 `Daoxin-Zhixi_*_x64.msi`。
- 应用内“设置与连接 → 检查更新”可安装后续签名更新。

当前版本与变更记录见 [CHANGELOG.md](CHANGELOG.md)。

首个公开版本尚未配置商业 Authenticode 证书，Windows SmartScreen 可能显示“未知发布者”。安装包仍由 GitHub Actions 从公开源码构建，并使用 Tauri 更新签名保证自动更新包的完整性。正式生产发行建议配置 OV/EV 代码签名证书。

## 核心能力

- assistant-ui 对话体验、Markdown/GFM 渲染和水稻胚乳科研品牌界面。
- Yuxi 异步 Agent run、SSE 增量输出、`Last-Event-ID` 断线续传和原任务结果恢复。
- 本地与服务端双重取消，不因网络重试创建重复 run。
- Stronghold 加密 vault + 操作系统凭据保险库保存 API Key；Key 不进入 SQLite、浏览器存储或日志。
- 用户可在设置页手动配置 OpenAI/Anthropic 兼容模型，或导入 Claude Code 风格 JSON；模型 API Key 只发送到 Yuxi 做用户级加密存储，不落本机数据库。
- SQLite 保存本地会话、消息和运行恢复状态。
- Tauri 2 原生 Windows 安装包和 GitHub Releases 自动更新。
- 远程地址强制 HTTPS，本机开发地址例外；严格 CSP 和最小 Tauri capability。
- 独立“科研工作流”域：本地项目沙箱、WISP 流式工具调用/审批/取消、运行历史、产物哈希与崩溃恢复不依赖 Yuxi 在线状态。
- 确定性表达矩阵 PCA 与显式 Artifact Bridge；本地产物只有经用户选择、完整性复核并在下一条问题发送时才进入 Yuxi。

## 双引擎边界

```text
智能问答：React → Tauri → APISIX/Yuxi（HTTPS/SSE）
科研工作流：React → Tauri WorkflowSupervisor → WISP Worker（stdio JSONL）
唯一跨域入口：用户显式触发的 Artifact Bridge
```

科研项目会创建 `input/`、`work/`、`results/`、`reports/`、`scripts/` 和 `.rice-workflow/`。确定性执行器只从 `input/` 读取，产物登记只接受 `results/` 与 `reports/` 内的真实普通文件；符号链接逃逸、父目录穿越、磁盘根和用户主目录均会被拒绝。工作流模型 API Key 保存在独立 Stronghold 记录中，不与 Yuxi 凭据或 SQLite 混用。

## 使用前提

你的 Yuxi/APISIX 实例需要开放以下经过 API Key 认证的接口：

- `POST /api/auth/desktop/login`：桌面端首次绑定时原子校验登录标识、密码和 API Key 归属（该端点自身不使用 Bearer Key，由网关限速保护）；

```text
GET  /api/agent-invocation/credential-status
GET  /api/agent/default
POST /api/chat/thread
POST /api/agent/runs
GET  /api/agent/runs/{run_id}/events?verbose=false
GET  /api/agent/runs/{run_id}/result
POST /api/agent/runs/{run_id}/cancel
```

从 v0.3.4 起，桌面端不再使用 `agent-call` 兼容包装。智能体、模型策略、知识范围、
会话历史和最终答案均由 Yuxi 原生 AgentRun 链路决定；桌面安装包中的
`YUXI_AGENT_SLUG` 只作为首次连接前的展示回退值，不参与已连接用户的实际问答路由。

配套 Yuxi 改造位于 [rice-endosperm-agent](https://github.com/yanmengli123/rice-endosperm-agent)。

## 本地开发

环境要求：Node.js 22+、pnpm 10、Rust stable、Tauri 2 的 Windows 构建依赖和 WebView2。

```powershell
git clone https://github.com/yanmengli123/rice-endosperm-desktop.git
cd rice-endosperm-desktop
pnpm install --frozen-lockfile
. .\.github\scripts\prepare-libsodium.ps1
.\.github\scripts\prepare-workflow-worker.ps1 -SourceDirectory "D:\path\to\rice-endosperm-workflow"
pnpm tauri dev
```

可通过编译期变量改变新安装用户看到的默认值：

```powershell
$env:YUXI_BASE_URL = "https://api.example.cn"
$env:YUXI_AGENT_SLUG = "default-chatbot"
pnpm tauri build --bundles nsis,msi
```

未配置变量时默认连接 `http://127.0.0.1:9088`。`YUXI_AGENT_SLUG` 仅决定连接服务端前的展示回退值；连接后始终以服务端默认智能体及线程绑定为准。变量中不包含任何 API Key。

## 质量检查

```powershell
pnpm check
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

Windows 首次编译 Stronghold 前请在当前 PowerShell 中点调用 `. .\.github\scripts\prepare-libsodium.ps1`。脚本会下载官方 libsodium 动态运行库、校验固定 SHA-256，并把 DLL 放入 Tauri 安装包资源；这种方式也避免静态 CRT 与 Rust MSVC 运行库冲突。开发模式可直接发现相邻 WISP fork 的 release/debug worker；正式安装包由 `prepare-workflow-worker.ps1` 从固定提交冷构建并生成来源/哈希清单。

## 数据与安全

- 应用不内置、收集或代管用户的 Yuxi API Key。
- 对话请求只发往用户配置的网关；本项目不包含遥测或第三方分析 SDK。
- 本地工作流文件不经过 Yuxi；但使用云端工作流模型时，Agent 选择的必要项目上下文会发送给该模型供应商。未发表或敏感数据应使用受信任的企业网关/本地模型，并逐项核对工具审批。
- Artifact Bridge 默认关闭；只有用户点击指定产物后才上传，且上传前会复核登记时的 SHA-256。
- 删除本机 API Key 不会删除本地历史会话；卸载策略与完整数据位置见 [隐私说明](PRIVACY.md)。
- 发现漏洞请按 [安全政策](SECURITY.md) 私下报告，不要在公开 Issue 中提交密钥或漏洞细节。

## 版本发布

版本标签触发 GitHub Actions 构建并发布安装包：

```powershell
git tag v0.1.6
git push origin v0.1.6
```

发布所需更新签名私钥只保存在 GitHub Actions Secrets 中，仓库仅提交公钥。完整流程见 [发布指南](docs/RELEASING.md)。

## 许可证与声明

桌面 Shell 使用 [MIT License](LICENSE)。Yuxi 是独立的开源项目。安装包内的 WISP Worker 是通过 JSONL stdio 协议运行的独立程序，采用 AGPL-3.0-only；其精确源码提交、二进制 SHA-256、许可证和源码链接随安装包提供。组合发行或商业部署仍应由发布方完成适用的开源许可证合规审查。“稻芯智析”不是医疗、农艺或实验决策的替代品，重要科研结论应核对原始文献并通过实验验证。
