# 双引擎科研工作台

日期：2026-08-30

## 目标

在不改变现有 Yuxi AgentRun 问答链路的前提下，为“稻芯智析”增加独立的本地科研工作流域：

- Yuxi 继续通过 HTTPS/SSE 提供知识问答。
- WISP 以独立 headless Sidecar 运行，通过版本化 JSONL RPC 接入。
- 本地项目、运行、产物、取消和凭据与问答域隔离。
- 第一条确定性工作流完成 `counts.csv -> PCA.csv + PCA.svg + report.md`。
- 两个引擎只通过用户显式确认的 Artifact Bridge 交换结果。

## 架构约束

- React 不获得 shell 或任意进程执行权限，只调用受控 Tauri commands。
- 工作流项目必须是明确授权的普通目录，拒绝磁盘根和用户主目录。
- 项目布局固定为 `input/`、`work/`、`results/`、`reports/`、`scripts/`、`.rice-workflow/`。
- `input/` 由受控执行器按只读输入处理；产物只写入 `results/` 和 `reports/`。
- QA SQLite 与 Workflow SQLite 分离；桌面端不直接读写 WISP 内部数据库。
- Sidecar stdout 只承载协议，stderr 只承载脱敏日志。
- 非幂等工具调用在进程崩溃后不得自动重放。

## 验收清单

- [x] 未配置 Yuxi 时仍可进入本地工作流。
- [x] 两个项目的数据、运行和产物相互隔离。
- [x] 路径穿越、符号链接逃逸、磁盘根和用户主目录被拒绝。
- [x] PCA 工作流生成带校验和的结构化产物与运行清单。
- [x] 切换问答/工作流不会取消另一域中的运行。
- [x] WISP 协议握手、流式事件、取消、审批和异常退出可恢复。
- [x] Rust、React 和 Sidecar 的相关测试通过。
- [x] 安装包中的 Worker 有固定版本、SHA-256、许可证和源码定位信息。

## 已落地的恢复与审计

- WISP 每个回合对应一条 `workflow_runs` 和 `workflow_agent_turns` 记录；异常退出在下次启动时转为 `interrupted`，不自动重放修改型工具。
- 只有 WISP provenance/file-changed 中位于 `results/` 或 `reports/` 的普通文件会登记为产物，并生成独立 manifest。
- Artifact Bridge 使用 append-only 审计事件记录 initiated/completed/failed，远端只接收用户明确选择且校验未变化的文件。
- 发布构建从固定 fork 提交生成 Worker，运行前校验随包 manifest 的协议与 SHA-256。

## 非代码门禁

- 当前安装包已包含 WISP `AGPL-3.0-only` 许可证、固定源码提交和源码定位；商业发行仍需发布方完成适用法律与许可证合规审查，或取得商业/双重授权。
- WSL2、SSH、GPU 与 HPC 验收依赖对应外部环境；自动化测试使用假执行器，不宣称未实机验证的能力。
