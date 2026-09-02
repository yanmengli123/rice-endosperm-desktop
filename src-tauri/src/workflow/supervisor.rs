use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tauri::ipc::Channel;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{ChildStdin, Command},
    sync::Mutex as AsyncMutex,
    time::{Instant, MissedTickBehavior},
};
use uuid::Uuid;

use crate::{
    diagnostics,
    error::{AppError, AppResult},
};

use super::{
    RICE_WORKFLOW_PROTOCOL, WispRpcEnvelope, WorkflowAgentCompletion, WorkflowAgentEvent,
    WorkflowEngineStatus, WorkflowModelSettings, WorkflowProject, artifacts::sha256_file,
};

const WISP_RPC_SCHEMA: &str = "wisp.agent-rpc.v1";
const MAX_RPC_FRAME_BYTES: usize = 4 * 1024 * 1024;
const MAX_VISIBLE_TOOL_RESULT_BYTES: usize = 16 * 1024;
const MAX_ACCUMULATED_TEXT_BYTES: usize = 2 * 1024 * 1024;
const DEFAULT_TURN_TIMEOUT_SECS: u64 = 2 * 60 * 60;
const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 3 * 60;
const DEFAULT_APPROVAL_TIMEOUT_SECS: u64 = 30 * 60;
const DEFAULT_FIRST_MODEL_EVENT_TIMEOUT_SECS: u64 = 2 * 60;
const PROGRESS_HEARTBEAT_SECS: u64 = 5;
const MIN_WORKFLOW_TIMEOUT_SECS: u64 = 30;
const MAX_WORKFLOW_TIMEOUT_SECS: u64 = 4 * 60 * 60;
const MAX_CONCURRENT_WORKFLOW_TASKS: usize = 6;
const MAX_CONCURRENT_TASKS_PER_PROJECT: usize = 3;

#[derive(Deserialize)]
struct WorkerBuildManifest {
    fork_commit: String,
    engine_version: String,
    sha256: String,
    resources_sha256: String,
    protocol: String,
}

struct ResolvedWorker {
    path: PathBuf,
    version: String,
    resource_root: Option<PathBuf>,
}

#[derive(Clone)]
struct ActiveWorker {
    project_id: String,
    stdin: Arc<AsyncMutex<ChildStdin>>,
    turn_id: String,
    cancelled: Arc<AtomicBool>,
}

#[derive(Clone)]
enum ActiveRun {
    Starting {
        project_id: String,
        cancelled: Arc<AtomicBool>,
    },
    Running(ActiveWorker),
}

impl ActiveRun {
    fn project_id(&self) -> &str {
        match self {
            Self::Starting { project_id, .. } => project_id,
            Self::Running(worker) => &worker.project_id,
        }
    }

    fn cancelled(&self) -> Arc<AtomicBool> {
        match self {
            Self::Starting { cancelled, .. } => cancelled.clone(),
            Self::Running(worker) => worker.cancelled.clone(),
        }
    }
}

pub struct WorkflowSupervisor {
    app_data_dir: PathBuf,
    active_workers: Mutex<HashMap<String, ActiveRun>>,
}

struct RunReservation<'a> {
    supervisor: &'a WorkflowSupervisor,
    run_id: String,
    cancelled: Arc<AtomicBool>,
}

impl Drop for RunReservation<'_> {
    fn drop(&mut self) {
        if let Ok(mut active) = self.supervisor.active_workers.lock() {
            active.remove(&self.run_id);
        }
    }
}

impl WorkflowSupervisor {
    pub fn new(app_data_dir: &Path) -> Self {
        Self {
            app_data_dir: app_data_dir.to_path_buf(),
            active_workers: Mutex::new(HashMap::new()),
        }
    }

    fn resolve_worker(&self) -> AppResult<Option<ResolvedWorker>> {
        let executable = if cfg!(windows) {
            "rice-workflow-worker.exe"
        } else {
            "rice-workflow-worker"
        };
        let wisp_executable = if cfg!(windows) {
            "wisp-science.exe"
        } else {
            "wisp-science"
        };
        let mut candidates: Vec<(PathBuf, Option<(PathBuf, PathBuf)>)> = Vec::new();
        if let Some(configured) = std::env::var_os("RICE_WORKFLOW_WORKER_PATH") {
            candidates.push((PathBuf::from(configured), None));
        }
        let managed = self
            .app_data_dir
            .join("workflow")
            .join("bin")
            .join(executable);
        candidates.push((
            managed.clone(),
            Some((
                managed.with_file_name("worker-build.json"),
                self.app_data_dir.join("workflow").join("engine"),
            )),
        ));
        if let Ok(current) = std::env::current_exe()
            && let Some(directory) = current.parent()
        {
            candidates.push((
                directory.join(executable),
                Some((
                    directory.join("workflow").join("worker-build.json"),
                    directory.join("workflow-engine"),
                )),
            ));
        }
        let desktop_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")));
        if let Some(workspace_root) = desktop_root.parent() {
            candidates.push((
                workspace_root
                    .join("wisp-science-main")
                    .join("target")
                    .join("release")
                    .join(wisp_executable),
                None,
            ));
            candidates.push((
                workspace_root
                    .join("wisp-science-main")
                    .join("target")
                    .join("debug")
                    .join(wisp_executable),
                None,
            ));
        }
        for (path, manifest) in candidates {
            if !path.is_file() {
                continue;
            }
            let Some((manifest_path, resource_root)) = manifest else {
                return Ok(Some(ResolvedWorker {
                    path,
                    version: "development-worker".into(),
                    resource_root: None,
                }));
            };
            if !manifest_path.is_file() {
                return Err(AppError::Protocol(format!(
                    "工作流 Worker 缺少构建清单：{}",
                    manifest_path.display()
                )));
            }
            let version = verify_worker_manifest(&path, &manifest_path, &resource_root)?;
            return Ok(Some(ResolvedWorker {
                path,
                version,
                resource_root: Some(resource_root),
            }));
        }
        Ok(None)
    }

    pub fn status(&self) -> WorkflowEngineStatus {
        let resolution = self.resolve_worker();
        let available = matches!(resolution, Ok(Some(_)));
        let running_projects = self
            .active_workers
            .lock()
            .map(|workers| workers.len())
            .unwrap_or_default();
        WorkflowEngineStatus {
            protocol: RICE_WORKFLOW_PROTOCOL.into(),
            available,
            running_projects,
            worker_path: resolution
                .as_ref()
                .ok()
                .and_then(|worker| worker.as_ref())
                .map(|worker| worker.path.to_string_lossy().into_owned()),
            worker_version: resolution
                .as_ref()
                .ok()
                .and_then(|worker| worker.as_ref())
                .map(|worker| worker.version.clone()),
            message: match resolution {
                Ok(Some(_)) => "WISP 本地引擎已安装且完整性校验通过；修改型工具逐项审批".into(),
                Ok(None) => "确定性工作流可用；WISP Agent Sidecar 尚未构建或安装".into(),
                Err(error) => format!("WISP Sidecar 已被阻止：{error}"),
            },
        }
    }

    pub async fn respond_approval(
        &self,
        run_id: &str,
        approval_id: &str,
        approved: bool,
        feedback: Option<&str>,
    ) -> AppResult<()> {
        let worker = self.active_worker(run_id)?;
        self.send(
            &worker,
            json!({
                "schema": WISP_RPC_SCHEMA,
                "id": format!("approval-{}", Uuid::new_v4().simple()),
                "type": "approval_response",
                "approval_id": approval_id,
                "approved": approved,
                "feedback": feedback,
            }),
        )
        .await
    }

    pub async fn cancel_turn(&self, run_id: &str) -> AppResult<bool> {
        let active = {
            let workers = self
                .active_workers
                .lock()
                .map_err(|_| AppError::Internal("WISP 进程状态锁已损坏".into()))?;
            workers.get(run_id).cloned()
        };
        let Some(active) = active else {
            return Ok(false);
        };
        active.cancelled().store(true, Ordering::Release);
        if let ActiveRun::Running(worker) = active {
            self.send(
                &worker,
                json!({
                    "schema": WISP_RPC_SCHEMA,
                    "id": format!("cancel-{}", Uuid::new_v4().simple()),
                    "type": "cancel",
                }),
            )
            .await?;
        }
        Ok(true)
    }

    fn active_worker(&self, run_id: &str) -> AppResult<ActiveWorker> {
        let active = self
            .active_workers
            .lock()
            .map_err(|_| AppError::Internal("WISP 进程状态锁已损坏".into()))?
            .get(run_id)
            .cloned()
            .ok_or_else(|| AppError::Internal("找不到对应的 WISP 运行任务".into()))?;
        match active {
            ActiveRun::Running(worker) => Ok(worker),
            ActiveRun::Starting { .. } => {
                Err(AppError::Internal("WISP 运行仍在启动，请稍后重试".into()))
            }
        }
    }

    fn reserve_run<'a>(&'a self, run_id: &str, project_id: &str) -> AppResult<RunReservation<'a>> {
        let mut active = self
            .active_workers
            .lock()
            .map_err(|_| AppError::Internal("WISP 进程状态锁已损坏".into()))?;
        if active.contains_key(run_id) {
            return Err(AppError::Internal("该科研任务已经启动".into()));
        }
        if active.len() >= MAX_CONCURRENT_WORKFLOW_TASKS {
            return Err(AppError::Internal(format!(
                "本机已有 {} 个科研任务在运行，请等待一个任务结束后重试",
                MAX_CONCURRENT_WORKFLOW_TASKS
            )));
        }
        let project_tasks = active
            .values()
            .filter(|run| run.project_id() == project_id)
            .count();
        if project_tasks >= MAX_CONCURRENT_TASKS_PER_PROJECT {
            return Err(AppError::Internal(format!(
                "当前项目已有 {} 个任务在运行；为保护本地资源，请等待一个任务结束后重试",
                MAX_CONCURRENT_TASKS_PER_PROJECT
            )));
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        active.insert(
            run_id.to_owned(),
            ActiveRun::Starting {
                project_id: project_id.to_owned(),
                cancelled: cancelled.clone(),
            },
        );
        Ok(RunReservation {
            supervisor: self,
            run_id: run_id.to_owned(),
            cancelled,
        })
    }

    fn activate_run(&self, run_id: &str, worker: ActiveWorker) -> AppResult<()> {
        let mut active = self
            .active_workers
            .lock()
            .map_err(|_| AppError::Internal("WISP 进程状态锁已损坏".into()))?;
        match active.get(run_id) {
            Some(ActiveRun::Starting { project_id, .. }) if project_id == &worker.project_id => {
                active.insert(run_id.to_owned(), ActiveRun::Running(worker));
                Ok(())
            }
            _ => Err(AppError::Internal("WISP 运行租约已经失效".into())),
        }
    }

    async fn send(&self, worker: &ActiveWorker, value: Value) -> AppResult<()> {
        let mut encoded = serde_json::to_vec(&value)
            .map_err(|error| AppError::Protocol(format!("WISP 命令编码失败：{error}")))?;
        encoded.push(b'\n');
        worker
            .stdin
            .lock()
            .await
            .write_all(&encoded)
            .await
            .map_err(|error| AppError::Protocol(format!("WISP 命令发送失败：{error}")))
    }

    pub async fn run_turn(
        &self,
        run_id: &str,
        project: &WorkflowProject,
        settings: &WorkflowModelSettings,
        api_key: SecretString,
        prompt: &str,
        channel: &Channel<WorkflowAgentEvent>,
    ) -> AppResult<WorkflowAgentCompletion> {
        let prompt = prompt.trim();
        if prompt.is_empty() || prompt.chars().count() > 100_000 {
            return Err(AppError::Protocol(
                "工作流指令必须为 1–100000 个字符".into(),
            ));
        }
        let turn_started_at = Instant::now();
        emit_progress(channel, "starting_engine", turn_started_at);
        let resolved_worker = self.resolve_worker()?.ok_or_else(|| {
            AppError::Internal("未找到 WISP Sidecar，请先构建或安装本地工作流引擎".into())
        })?;
        let worker_path = resolved_worker.path;
        let reservation = self.reserve_run(run_id, &project.id)?;
        let isolated_output = format!("results/{run_id}");
        let worker_prompt = isolated_worker_prompt(run_id, &isolated_output, prompt);

        let mut command = Command::new(&worker_path);
        command
            .arg("rpc")
            .current_dir(&project.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .env("WISP_PROVIDER", &settings.provider)
            .env("WISP_API_URL", &settings.base_url)
            .env("WISP_MODEL", &settings.model)
            .env("WISP_API_KEY", api_key.expose_secret())
            .env("WISP_RUN_ID", run_id)
            .env("WISP_RUN_OUTPUT_DIR", &isolated_output)
            .env("WISP_APPROVAL_MODE", "safe")
            .env("WISP_RESTRICT_READS", "1")
            .env("WISP_MAX_ITER", "60")
            // The worker and all of its Python/PowerShell descendants must
            // exchange UTF-8 bytes.  Without this, Windows' legacy console
            // code page can corrupt Chinese scientific text or raise
            // UnicodeEncodeError in otherwise successful tools.
            .env("PYTHONUTF8", "1")
            .env("PYTHONIOENCODING", "utf-8")
            .env("LANG", "C.UTF-8")
            .env("LC_ALL", "C.UTF-8")
            .env("NO_COLOR", "1");
        if let Some(resource_root) = resolved_worker.resource_root {
            command.env("WISP_RESOURCE_ROOT", resource_root);
        }
        hide_console(&mut command);
        let mut child = command.spawn().map_err(|error| {
            AppError::Internal(format!(
                "无法启动 WISP Sidecar {}：{error}",
                worker_path.display()
            ))
        })?;
        let stdin = Arc::new(AsyncMutex::new(
            child
                .stdin
                .take()
                .ok_or_else(|| AppError::Internal("WISP stdin 不可用".into()))?,
        ));
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AppError::Internal("WISP stdout 不可用".into()))?;
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                let mut count = 0_u64;
                let mut retries = 0_u64;
                let mut errors = 0_u64;
                while let Ok(Some(line)) = lines.next_line().await {
                    count = count.saturating_add(1);
                    let normalized = line.to_ascii_lowercase();
                    if normalized.contains("retry") {
                        retries = retries.saturating_add(1);
                    }
                    if normalized.contains("error") || normalized.contains("failed") {
                        errors = errors.saturating_add(1);
                    }
                }
                if count > 0 {
                    diagnostics::log(
                        "INFO",
                        "workflow_worker_stderr",
                        &format!(
                            "worker emitted {count} diagnostic lines (retries={retries}, errors={errors}); content suppressed"
                        ),
                    );
                }
            });
        }
        let mut lines = BufReader::new(stdout).lines();
        let ready = tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let line = lines
                    .next_line()
                    .await
                    .map_err(|error| AppError::Protocol(format!("WISP 握手读取失败：{error}")))?
                    .ok_or_else(|| AppError::Protocol("WISP 在握手前退出".into()))?;
                let event = parse_frame(&line)?;
                if event.event_type == "startup_error" {
                    return Err(AppError::Protocol(payload_string(&event, "message")));
                }
                if event.event_type == "ready" {
                    return Ok(event);
                }
            }
        })
        .await
        .map_err(|_| AppError::Protocol("WISP 启动握手超时".into()))??;
        let _ = channel.send(WorkflowAgentEvent::EngineReady {
            protocol: ready.schema.clone(),
            model: payload_string(&ready, "model"),
            root: payload_string(&ready, "root"),
        });
        emit_progress(channel, "waiting_model", turn_started_at);

        let turn_id = format!("turn-{}", Uuid::new_v4().simple());
        let worker = ActiveWorker {
            project_id: project.id.clone(),
            stdin,
            turn_id: turn_id.clone(),
            cancelled: reservation.cancelled.clone(),
        };
        if reservation.cancelled.load(Ordering::Acquire) {
            let _ = child.kill().await;
            return Err(AppError::Cancelled);
        }
        self.activate_run(run_id, worker.clone())?;
        if let Err(error) = self
            .send(
                &worker,
                json!({
                    "schema": WISP_RPC_SCHEMA,
                    "id": turn_id,
                    "type": "prompt",
                    "prompt": worker_prompt,
                }),
            )
            .await
        {
            let _ = child.kill().await;
            return Err(error);
        }

        let mut text = String::new();
        let mut session_id = ready.session_id;
        let mut last_sequence = ready.sequence;
        let mut reasoning_announced = false;
        let mut changed_paths = HashSet::new();
        let mut input_tokens = 0_u64;
        let mut output_tokens = 0_u64;
        let mut reasoning_tokens = 0_u64;
        let total_timeout =
            workflow_timeout("RICE_WORKFLOW_TURN_TIMEOUT_SECS", DEFAULT_TURN_TIMEOUT_SECS);
        let idle_timeout =
            workflow_timeout("RICE_WORKFLOW_IDLE_TIMEOUT_SECS", DEFAULT_IDLE_TIMEOUT_SECS);
        let approval_timeout = workflow_timeout(
            "RICE_WORKFLOW_APPROVAL_TIMEOUT_SECS",
            DEFAULT_APPROVAL_TIMEOUT_SECS,
        );
        let first_model_event_timeout = workflow_timeout(
            "RICE_WORKFLOW_FIRST_EVENT_TIMEOUT_SECS",
            DEFAULT_FIRST_MODEL_EVENT_TIMEOUT_SECS,
        );
        let total_deadline = turn_started_at + total_timeout;
        let first_model_event_deadline = turn_started_at + first_model_event_timeout;
        let mut last_worker_event_at = Instant::now();
        let mut received_model_activity = false;
        let mut phase = "waiting_model";
        let mut heartbeat = tokio::time::interval(Duration::from_secs(PROGRESS_HEARTBEAT_SECS));
        heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);
        heartbeat.tick().await;
        let result = loop {
            let now = Instant::now();
            if now >= total_deadline {
                break Err(AppError::Protocol(format!(
                    "WISP 工作流超过总时限（{} 秒），已安全终止；中间文件仍保留在项目目录",
                    total_timeout.as_secs()
                )));
            }
            let effective_idle_timeout = if phase == "waiting_approval" {
                approval_timeout
            } else {
                idle_timeout
            };
            let idle_deadline = last_worker_event_at + effective_idle_timeout;
            let mut read_deadline = total_deadline.min(idle_deadline);
            let mut timeout_message = if total_deadline <= idle_deadline {
                format!(
                    "WISP 工作流超过总时限（{} 秒），已安全终止；中间文件仍保留在项目目录",
                    total_timeout.as_secs()
                )
            } else {
                format!(
                    "WISP 工作流连续 {} 秒没有产生协议事件，已按卡死保护终止",
                    effective_idle_timeout.as_secs()
                )
            };
            if !received_model_activity && first_model_event_deadline < read_deadline {
                read_deadline = first_model_event_deadline;
                timeout_message = format!(
                    "模型在 {} 秒内没有返回文本、推理或工具调用，已终止本轮；请检查模型服务、网络或 Base URL",
                    first_model_event_timeout.as_secs()
                );
            }
            let line = tokio::select! {
                next_line = lines.next_line() => match next_line {
                    Ok(Some(line)) => line,
                    Ok(None) => break Err(AppError::Protocol("WISP 在完成指令前退出".into())),
                    Err(error) => {
                        break Err(AppError::Protocol(format!("WISP 流读取失败：{error}")));
                    }
                },
                _ = tokio::time::sleep_until(read_deadline) => {
                    break Err(AppError::Protocol(timeout_message));
                },
                _ = heartbeat.tick() => {
                    emit_progress(channel, phase, turn_started_at);
                    continue;
                }
            };
            if line.trim().is_empty() {
                continue;
            }
            let event = match parse_frame(&line) {
                Ok(event) => event,
                Err(_) => {
                    diagnostics::log(
                        "WARN",
                        "wisp_frame_skipped",
                        &format!("run={}: non-protocol stdout line", turn_id),
                    );
                    continue;
                }
            };
            last_worker_event_at = Instant::now();
            if let (Some(previous), Some(current)) = (last_sequence, event.sequence)
                && current <= previous
            {
                break Err(AppError::Protocol("WISP 事件序列发生回退或重复".into()));
            }
            last_sequence = event.sequence.or(last_sequence);
            session_id = event.session_id.clone().or(session_id);
            match event.event_type.as_str() {
                "turn_started" => {
                    phase = "waiting_model";
                    let _ = channel.send(WorkflowAgentEvent::TurnStarted {
                        turn_id: worker.turn_id.clone(),
                    });
                    emit_progress(channel, phase, turn_started_at);
                }
                "text" => {
                    received_model_activity = true;
                    phase = "streaming_answer";
                    let delta = payload_string(&event, "delta");
                    if text.len().saturating_add(delta.len()) > MAX_ACCUMULATED_TEXT_BYTES {
                        break Err(AppError::Protocol("WISP 回答超过 2 MB 安全上限".into()));
                    }
                    text.push_str(&delta);
                    let _ = channel.send(WorkflowAgentEvent::TextDelta { delta });
                }
                "reasoning" => {
                    received_model_activity = true;
                    phase = "reasoning";
                    if !reasoning_announced {
                        reasoning_announced = true;
                        let _ = channel.send(WorkflowAgentEvent::ReasoningActive);
                        emit_progress(channel, phase, turn_started_at);
                    }
                }
                "tool_call" => {
                    received_model_activity = true;
                    phase = "running_tool";
                    emit_progress(channel, phase, turn_started_at);
                    let _ = channel.send(WorkflowAgentEvent::ToolStarted {
                        call_id: payload_optional_string(&event, "call_id"),
                        name: payload_string(&event, "name"),
                        preview: payload_string(&event, "preview"),
                    });
                }
                "tool_result" => {
                    phase = "verifying_result";
                    let content = truncate_utf8(
                        &payload_string(&event, "content"),
                        MAX_VISIBLE_TOOL_RESULT_BYTES,
                    );
                    let _ = channel.send(WorkflowAgentEvent::ToolFinished {
                        call_id: payload_optional_string(&event, "call_id"),
                        name: payload_string(&event, "name"),
                        ok: payload_bool(&event, "ok"),
                        content,
                        duration_ms: payload_u64(&event, "duration_ms"),
                    });
                    emit_progress(channel, phase, turn_started_at);
                }
                "approval_required" => {
                    received_model_activity = true;
                    phase = "waiting_approval";
                    let _ = channel.send(WorkflowAgentEvent::ApprovalRequired {
                        approval_id: payload_string(&event, "approval_id"),
                        message: payload_string(&event, "message"),
                    });
                    emit_progress(channel, phase, turn_started_at);
                }
                "approval_response_accepted" => {
                    phase = "running_tool";
                    emit_progress(channel, phase, turn_started_at);
                }
                "file_changed" => {
                    let path = payload_string(&event, "path");
                    if !path.trim().is_empty() {
                        changed_paths.insert(path.clone());
                    }
                    let _ = channel.send(WorkflowAgentEvent::FileChanged { path });
                }
                "provenance" => {
                    for path in payload_string_array(&event, "files_written") {
                        if !path.trim().is_empty() {
                            changed_paths.insert(path);
                        }
                    }
                }
                "usage" => {
                    input_tokens = payload_u64(&event, "input_tokens");
                    output_tokens = payload_u64(&event, "output_tokens");
                    reasoning_tokens = payload_u64(&event, "reasoning_tokens");
                    let _ = channel.send(WorkflowAgentEvent::Usage {
                        input_tokens,
                        output_tokens,
                        reasoning_tokens,
                    });
                }
                "turn_completed" => {
                    let ok = payload_bool(&event, "ok");
                    let error = payload_optional_string(&event, "error");
                    let _ = channel.send(WorkflowAgentEvent::TurnCompleted {
                        ok,
                        error: error.clone(),
                    });
                    if ok {
                        break Ok(());
                    }
                    break Err(AppError::Protocol(
                        error.unwrap_or_else(|| "WISP 指令执行失败".into()),
                    ));
                }
                "protocol_error" | "command_error" => {
                    break Err(AppError::Protocol(payload_string(&event, "message")));
                }
                _ => {}
            }
        };

        let _ = self
            .send(
                &worker,
                json!({
                    "schema": WISP_RPC_SCHEMA,
                    "id": format!("shutdown-{}", Uuid::new_v4().simple()),
                    "type": "shutdown",
                }),
            )
            .await;
        if tokio::time::timeout(Duration::from_secs(3), child.wait())
            .await
            .is_err()
        {
            let _ = child.kill().await;
        }
        let result = if worker.cancelled.load(Ordering::Acquire) {
            Err(AppError::Cancelled)
        } else {
            result
        };
        if let Err(error) = &result {
            let _ = channel.send(WorkflowAgentEvent::EngineError {
                message: error.to_string(),
            });
        }
        result?;
        diagnostics::log(
            "INFO",
            "workflow_turn_completed",
            &format!(
                "elapsed_ms={} input_tokens={} output_tokens={} changed_paths={}",
                turn_started_at.elapsed().as_millis(),
                input_tokens,
                output_tokens,
                changed_paths.len()
            ),
        );
        Ok(WorkflowAgentCompletion {
            turn_id: worker.turn_id,
            text,
            session_id,
            input_tokens,
            output_tokens,
            reasoning_tokens,
            changed_paths: changed_paths.into_iter().collect(),
        })
    }
}

fn isolated_worker_prompt(run_id: &str, output_directory: &str, user_prompt: &str) -> String {
    format!(
        "[稻芯智析本地任务边界]\n任务 ID：{run_id}\n这是一个可与同项目其他任务并行的独立任务。读取项目文件时保持只读；新建结果默认写入 {output_directory}/，不得覆盖其他任务的输出。只有用户明确指定共享目标且完成冲突核对与操作审批后，才可以写入共享路径。\n\n[用户任务]\n{user_prompt}"
    )
}

fn emit_progress(channel: &Channel<WorkflowAgentEvent>, phase: &str, started_at: Instant) {
    let message = match phase {
        "starting_engine" => "正在启动并校验本地科研引擎…",
        "waiting_model" => "本地引擎已就绪，正在等待模型响应…",
        "reasoning" => "模型正在规划科研任务…",
        "streaming_answer" => "模型正在输出结果…",
        "running_tool" => "正在调用本地科研工具…",
        "verifying_result" => "工具执行完成，正在核验结果…",
        "waiting_approval" => "工作流正在等待你的操作授权…",
        _ => "科研工作流正在运行…",
    };
    let _ = channel.send(WorkflowAgentEvent::Progress {
        phase: phase.to_owned(),
        message: message.into(),
        elapsed_ms: started_at
            .elapsed()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX),
    });
}

fn resource_tree_digest(resource_root: &Path) -> AppResult<String> {
    for required in ["skills", "python", "r", "seed", "mcp-servers/bio-tools"] {
        if !resource_root.join(required).is_dir() {
            return Err(AppError::Protocol(format!(
                "工作流 Worker 缺少运行资源：{required}"
            )));
        }
    }
    fn collect(root: &Path, directory: &Path, files: &mut Vec<PathBuf>) -> AppResult<()> {
        for entry in std::fs::read_dir(directory)
            .map_err(|error| AppError::Protocol(format!("无法读取工作流资源：{error}")))?
        {
            let entry = entry
                .map_err(|error| AppError::Protocol(format!("无法读取工作流资源：{error}")))?;
            let metadata = entry
                .file_type()
                .map_err(|error| AppError::Protocol(format!("无法校验工作流资源：{error}")))?;
            if metadata.is_symlink() {
                return Err(AppError::Protocol(
                    "工作流运行资源中不允许包含符号链接".into(),
                ));
            }
            if metadata.is_dir() {
                collect(root, &entry.path(), files)?;
            } else if metadata.is_file() {
                let relative = entry
                    .path()
                    .strip_prefix(root)
                    .map_err(|_| AppError::Protocol("工作流资源越过安装目录".into()))?
                    .to_path_buf();
                files.push(relative);
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    collect(resource_root, resource_root, &mut files)?;
    files.sort_by(|left, right| {
        left.to_string_lossy()
            .replace('\\', "/")
            .cmp(&right.to_string_lossy().replace('\\', "/"))
    });
    let mut digest = Sha256::new();
    for relative in files {
        let normalized = relative.to_string_lossy().replace('\\', "/");
        let (_, file_hash) = sha256_file(&resource_root.join(&relative))?;
        digest.update(normalized.as_bytes());
        digest.update(b"\t");
        digest.update(file_hash.as_bytes());
        digest.update(b"\n");
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn verify_worker_manifest(
    worker: &Path,
    manifest_path: &Path,
    resource_root: &Path,
) -> AppResult<String> {
    let bytes = std::fs::read(manifest_path)
        .map_err(|error| AppError::Protocol(format!("无法读取工作流 Worker 清单：{error}")))?;
    let manifest: WorkerBuildManifest = serde_json::from_slice(&bytes)
        .map_err(|error| AppError::Protocol(format!("工作流 Worker 清单格式无效：{error}")))?;
    if manifest.protocol != WISP_RPC_SCHEMA {
        return Err(AppError::Protocol("工作流 Worker 协议版本不匹配".into()));
    }
    let (_, actual_hash) = sha256_file(worker)?;
    if !actual_hash.eq_ignore_ascii_case(&manifest.sha256) {
        return Err(AppError::Protocol(
            "工作流 Worker 完整性校验失败；请重新安装官方发布包".into(),
        ));
    }
    let actual_resources_hash = resource_tree_digest(resource_root)?;
    if !actual_resources_hash.eq_ignore_ascii_case(&manifest.resources_sha256) {
        return Err(AppError::Protocol(
            "工作流 Worker 运行资源完整性校验失败；请重新安装官方发布包".into(),
        ));
    }
    Ok(format!(
        "{} @ {}",
        manifest.engine_version,
        manifest.fork_commit.chars().take(12).collect::<String>()
    ))
}

fn parse_frame(line: &str) -> AppResult<WispRpcEnvelope> {
    if line.len() > MAX_RPC_FRAME_BYTES {
        return Err(AppError::Protocol("WISP RPC 单帧超过 4 MB 安全上限".into()));
    }
    let event = serde_json::from_str::<WispRpcEnvelope>(line)
        .map_err(|error| AppError::Protocol(format!("WISP 返回了无效 JSON：{error}")))?;
    if event.schema != WISP_RPC_SCHEMA {
        return Err(AppError::Protocol(format!(
            "WISP 协议不兼容：期望 {WISP_RPC_SCHEMA}，实际 {}",
            event.schema
        )));
    }
    Ok(event)
}

fn payload_string(event: &WispRpcEnvelope, key: &str) -> String {
    event
        .payload
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn payload_optional_string(event: &WispRpcEnvelope, key: &str) -> Option<String> {
    event
        .payload
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn payload_u64(event: &WispRpcEnvelope, key: &str) -> u64 {
    event.payload.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn payload_bool(event: &WispRpcEnvelope, key: &str) -> bool {
    event
        .payload
        .get(key)
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn payload_string_array(event: &WispRpcEnvelope, key: &str) -> Vec<String> {
    event
        .payload
        .get(key)
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n…[工具结果已截断]", &value[..end])
}

fn bounded_timeout_secs(raw: Option<&str>, default_secs: u64) -> Duration {
    let seconds = raw
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(default_secs)
        .clamp(MIN_WORKFLOW_TIMEOUT_SECS, MAX_WORKFLOW_TIMEOUT_SECS);
    Duration::from_secs(seconds)
}

fn workflow_timeout(name: &str, default_secs: u64) -> Duration {
    bounded_timeout_secs(std::env::var(name).ok().as_deref(), default_secs)
}

#[cfg(windows)]
fn hide_console(command: &mut Command) {
    command.creation_flags(0x0800_0000);
}

#[cfg(not(windows))]
fn hide_console(_command: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_the_expected_versioned_protocol() {
        let event = parse_frame(
            r#"{"schema":"wisp.agent-rpc.v1","type":"ready","sequence":0,"model":"m"}"#,
        )
        .unwrap();
        assert_eq!(event.event_type, "ready");
        assert!(parse_frame(r#"{"schema":"wisp.agent-rpc.v0","type":"ready"}"#).is_err());
    }

    #[test]
    fn truncation_preserves_utf8_boundaries() {
        let value = "水稻胚乳".repeat(10_000);
        let truncated = truncate_utf8(&value, 101);
        assert!(truncated.is_char_boundary(truncated.len()));
        assert!(truncated.contains("已截断"));
    }

    #[test]
    fn workflow_timeouts_are_bounded_and_invalid_values_use_defaults() {
        assert_eq!(bounded_timeout_secs(Some("5"), 90).as_secs(), 30);
        assert_eq!(bounded_timeout_secs(Some("999999"), 90).as_secs(), 14_400);
        assert_eq!(bounded_timeout_secs(Some("invalid"), 90).as_secs(), 90);
        assert_eq!(bounded_timeout_secs(None, 90).as_secs(), 90);
    }

    #[test]
    fn run_reservations_allow_controlled_parallel_tasks_and_release_on_drop() {
        let supervisor = WorkflowSupervisor::new(Path::new("."));
        let first = supervisor.reserve_run("run-1", "project-1").unwrap();
        let second = supervisor.reserve_run("run-2", "project-1").unwrap();
        let third = supervisor.reserve_run("run-3", "project-1").unwrap();
        assert!(supervisor.reserve_run("run-4", "project-1").is_err());
        drop(first);
        assert!(supervisor.reserve_run("run-4", "project-1").is_ok());
        drop((second, third));
    }

    #[test]
    fn run_reservations_enforce_global_limit_and_unique_run_ids() {
        let supervisor = WorkflowSupervisor::new(Path::new("."));
        let mut reservations = Vec::new();
        for index in 0..MAX_CONCURRENT_WORKFLOW_TASKS {
            reservations.push(
                supervisor
                    .reserve_run(&format!("run-{index}"), &format!("project-{}", index / 3))
                    .unwrap(),
            );
        }
        assert!(supervisor.reserve_run("run-overflow", "project-3").is_err());
        assert!(supervisor.reserve_run("run-0", "project-0").is_err());
        drop(reservations.pop());
        assert!(supervisor.reserve_run("run-released", "project-3").is_ok());
    }

    #[test]
    fn worker_prompt_keeps_user_request_and_assigns_an_isolated_output_directory() {
        let prompt = isolated_worker_prompt(
            "wfr_0123456789abcdef0123456789abcdef",
            "results/wfr_0123456789abcdef0123456789abcdef",
            "hi",
        );
        assert!(prompt.ends_with("[用户任务]\nhi"));
        assert!(prompt.contains("不得覆盖其他任务的输出"));
        assert!(prompt.contains("results/wfr_0123456789abcdef0123456789abcdef/"));
    }

    #[test]
    fn bundled_worker_manifest_is_fail_closed_on_tampering() {
        let root = std::env::temp_dir().join(format!("rice-worker-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let worker = root.join("rice-workflow-worker.exe");
        let manifest = root.join("worker-build.json");
        let resources = root.join("resources");
        for directory in ["skills", "python", "r", "seed", "mcp-servers/bio-tools"] {
            std::fs::create_dir_all(resources.join(directory)).unwrap();
        }
        std::fs::write(resources.join("skills/catalog.txt"), b"verified resources").unwrap();
        std::fs::write(&worker, b"verified worker").unwrap();
        let (_, digest) = sha256_file(&worker).unwrap();
        let resources_digest = resource_tree_digest(&resources).unwrap();
        std::fs::write(
            &manifest,
            serde_json::json!({
                "fork_commit": "1234567890abcdef",
                "engine_version": "wisp 1.8.0",
                "sha256": digest,
                "resources_sha256": resources_digest,
                "protocol": WISP_RPC_SCHEMA,
            })
            .to_string(),
        )
        .unwrap();
        assert!(verify_worker_manifest(&worker, &manifest, &resources).is_ok());
        std::fs::write(&worker, b"tampered worker").unwrap();
        assert!(verify_worker_manifest(&worker, &manifest, &resources).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn prepared_release_worker_and_resource_tree_match_manifest() {
        let resources = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
        let worker = resources.join("rice-workflow-worker.exe");
        let manifest = resources.join("workflow/worker-build.json");
        let engine = resources.join("workflow-engine");
        if !worker.is_file() || !manifest.is_file() || !engine.is_dir() {
            eprintln!("skip: release worker resources have not been prepared");
            return;
        }
        verify_worker_manifest(&worker, &manifest, &engine)
            .expect("prepared worker and resource tree must match the release manifest");
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn prepared_release_worker_starts_with_bundled_resource_root() {
        let resources = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
        let worker = resources.join("rice-workflow-worker.exe");
        let manifest = resources.join("workflow/worker-build.json");
        let engine = resources.join("workflow-engine");
        if !worker.is_file() || !manifest.is_file() || !engine.is_dir() {
            eprintln!("skip: release worker resources have not been prepared");
            return;
        }
        verify_worker_manifest(&worker, &manifest, &engine)
            .expect("prepared worker assets must pass integrity validation");
        let project =
            std::env::temp_dir().join(format!("rice-wf-bundle-{}", Uuid::new_v4().simple()));
        std::fs::create_dir_all(project.join("input")).unwrap();
        let mut command = Command::new(&worker);
        command
            .arg("rpc")
            .current_dir(&project)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .env("WISP_RESOURCE_ROOT", &engine)
            .env("WISP_PROVIDER", "anthropic")
            .env("WISP_API_URL", "http://127.0.0.1:9")
            .env("WISP_MODEL", "smoke-model")
            .env("WISP_API_KEY", "sk-smoke-not-real")
            .env("WISP_APPROVAL_MODE", "safe")
            .env("WISP_RESTRICT_READS", "1");
        let mut child = command.spawn().expect("prepared worker spawn");
        let stdout = child.stdout.take().unwrap();
        let mut stdout = BufReader::new(stdout);
        let ready = tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let mut line = String::new();
                stdout.read_line(&mut line).await.expect("read ready frame");
                assert!(!line.is_empty(), "prepared worker closed before ready");
                if line.trim().is_empty() {
                    continue;
                }
                let event = parse_frame(line.trim()).expect("prepared worker frame must parse");
                if event.event_type == "ready" {
                    return event;
                }
            }
        })
        .await
        .expect("prepared worker ready timeout");
        assert_eq!(ready.schema, WISP_RPC_SCHEMA);
        let _ = child.kill().await;
        let _ = std::fs::remove_dir_all(project);
    }

    /// 真实 Sidecar 协议冒烟：spawn 固定提交构建的 WISP worker，验证
    /// ready 握手 → prompt 接受 → turn 生命周期完整走通。伪造模型端点
    /// （127.0.0.1:9）不消耗真实模型；worker 未构建时跳过（CI Linux）。
    #[cfg(windows)]
    #[tokio::test]
    async fn real_worker_protocol_framing_smoke() {
        let desktop_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let workspace_root = desktop_root.parent().unwrap();
        let worker = workspace_root
            .join("wisp-science-main")
            .join("target")
            .join("release")
            .join("wisp-science.exe");
        if !worker.exists() {
            eprintln!("skip: WISP worker not built at {}", worker.display());
            return;
        }

        let project = std::env::temp_dir().join(format!("rice-wf-it-{}", Uuid::new_v4().simple()));
        std::fs::create_dir_all(project.join("input")).unwrap();

        let mut command = Command::new(&worker);
        command
            .arg("rpc")
            .current_dir(&project)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .env("WISP_PROVIDER", "anthropic")
            .env("WISP_API_URL", "http://127.0.0.1:9")
            .env("WISP_MODEL", "smoke-model")
            .env("WISP_API_KEY", "sk-smoke-not-real")
            .env("WISP_APPROVAL_MODE", "safe")
            .env("WISP_RESTRICT_READS", "1")
            .env("WISP_MAX_ITER", "3");
        let mut child = command.spawn().expect("worker spawn");
        let mut stdin = child.stdin.take().unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());

        // 1) ready 握手
        let ready = tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let mut line = String::new();
                stdout.read_line(&mut line).await.expect("read ready frame");
                assert!(!line.is_empty(), "worker stdout closed before ready");
                let event = parse_frame(line.trim()).expect("worker frame must parse");
                if event.event_type == "ready" {
                    return event;
                }
            }
        })
        .await
        .expect("ready timeout");
        assert_eq!(ready.schema, WISP_RPC_SCHEMA);
        assert_eq!(ready.sequence, Some(0));

        // 2) prompt 帧
        let prompt = json!({
            "schema": WISP_RPC_SCHEMA,
            "id": "it-turn-1",
            "type": "prompt",
            "prompt": "integration smoke",
        });
        stdin
            .write_all(&serde_json::to_vec(&prompt).unwrap())
            .await
            .unwrap();
        stdin.write_all(b"\n").await.unwrap();
        stdin.flush().await.unwrap();

        // 3) prompt 已由 worker 接受并进入 turn 生命周期。
        let mut saw_turn_started = false;
        // 只验证本产品依赖的 framing/turn_started 契约。伪端点上的模型重试
        // 属于 worker 内部策略，不能把桌面端 CI 阻塞数分钟。
        let _ = tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let mut line = String::new();
                let read = stdout.read_line(&mut line).await.expect("read turn frame");
                if read == 0 {
                    break;
                }
                if line.trim().is_empty() {
                    continue;
                }
                let event = match parse_frame(line.trim()) {
                    Ok(event) => event,
                    Err(_) => {
                        eprintln!("SMOKE non-json frame: {:?}", line);
                        continue;
                    }
                };
                if event.event_type == "turn_started" {
                    saw_turn_started = true;
                    break;
                }
            }
        })
        .await;
        assert!(saw_turn_started, "turn_started not observed");

        let _ = child.kill().await;
        let _ = std::fs::remove_dir_all(project);
    }
}
