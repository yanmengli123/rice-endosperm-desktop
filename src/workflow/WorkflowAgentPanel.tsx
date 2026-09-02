import { Channel } from "@tauri-apps/api/core";
import {
  Bot,
  CheckCircle2,
  CircleAlert,
  Clock3,
  KeyRound,
  Layers3,
  LoaderCircle,
  MessageSquarePlus,
  Play,
  Save,
  Settings2,
  ShieldQuestion,
  Square,
  TerminalSquare,
  Trash2,
  X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import {
  cancelWorkflowAgent,
  deleteWorkflowModelSettings,
  getWorkflowModelSettings,
  listWorkflowAgentTurns,
  normalizeCommandError,
  respondWorkflowApproval,
  runWorkflowAgent,
  saveWorkflowModelSettings,
} from "../services/tauri-client";
import type {
  WorkflowAgentEvent,
  WorkflowAgentTurn,
  WorkflowEngineStatus,
  WorkflowModelSettings,
  WorkflowProject,
} from "../types";

type Props = {
  project: WorkflowProject;
  engine?: WorkflowEngineStatus;
  focusRunId?: string;
  focusRequest?: number;
  onFilesChanged: () => void;
  onTaskActivity?: () => void;
};

type ToolActivity = {
  id: string;
  name: string;
  preview: string;
  status: "running" | "completed" | "failed";
  content?: string;
};

type Approval = { id: string; message: string };

type LiveTask = {
  runId: string;
  projectId: string;
  prompt: string;
  answer: string;
  status: WorkflowAgentTurn["status"];
  model: string;
  createdAt: string;
  reasoning: boolean;
  progressMessage: string;
  tools: ToolActivity[];
  approval?: Approval;
  error?: string;
};

type TaskListItem = {
  runId: string;
  prompt: string;
  model: string;
  status: WorkflowAgentTurn["status"];
  createdAt: string;
  progressMessage?: string;
};

const DEEPSEEK_SETTINGS: WorkflowModelSettings = {
  provider: "openai",
  baseUrl: "https://api.deepseek.com",
  model: "deepseek-chat",
  hasApiKey: false,
};

function createWorkflowRunId() {
  const cryptoApi = globalThis.crypto;
  if (typeof cryptoApi?.randomUUID === "function") {
    return `wfr_${cryptoApi.randomUUID().replace(/-/g, "")}`;
  }
  const bytes = new Uint8Array(16);
  cryptoApi?.getRandomValues(bytes);
  if (!bytes.some(Boolean)) {
    for (let index = 0; index < bytes.length; index += 1) {
      bytes[index] = Math.floor(Math.random() * 256);
    }
  }
  return `wfr_${Array.from(bytes, (value) => value.toString(16).padStart(2, "0")).join("")}`;
}

function statusLabel(status: WorkflowAgentTurn["status"]) {
  return {
    running: "运行中",
    completed: "已完成",
    failed: "失败",
    cancelled: "已取消",
    interrupted: "已中断",
  }[status];
}

function isDeepSeek(settings?: WorkflowModelSettings | null) {
  return settings?.provider === "openai"
    && settings.baseUrl.replace(/\/$/, "") === DEEPSEEK_SETTINGS.baseUrl
    && settings.model === DEEPSEEK_SETTINGS.model;
}

export function WorkflowAgentPanel({
  project,
  engine,
  focusRunId,
  focusRequest,
  onFilesChanged,
  onTaskActivity,
}: Props) {
  const cardRef = useRef<HTMLElement>(null);
  const [settings, setSettings] = useState<WorkflowModelSettings | null>();
  const [editing, setEditing] = useState(false);
  const [provider, setProvider] = useState<WorkflowModelSettings["provider"]>("openai");
  const [baseUrl, setBaseUrl] = useState(DEEPSEEK_SETTINGS.baseUrl);
  const [model, setModel] = useState(DEEPSEEK_SETTINGS.model);
  const [apiKey, setApiKey] = useState("");
  const [prompt, setPrompt] = useState("");
  const [globalError, setGlobalError] = useState("");
  const [history, setHistory] = useState<WorkflowAgentTurn[]>([]);
  const [liveTasks, setLiveTasks] = useState<LiveTask[]>([]);
  const [selectedRunId, setSelectedRunId] = useState<string>();

  const refreshHistory = useCallback(async () => {
    const turns = await listWorkflowAgentTurns(project.id);
    setHistory(turns);
    return turns;
  }, [project.id]);

  const projectLiveTasks = useMemo(
    () => liveTasks.filter((task) => task.projectId === project.id),
    [liveTasks, project.id],
  );
  const liveRunIds = useMemo(
    () => new Set(projectLiveTasks.map((task) => task.runId)),
    [projectLiveTasks],
  );
  const taskItems = useMemo<TaskListItem[]>(() => [
    ...projectLiveTasks.map((task) => ({
      runId: task.runId,
      prompt: task.prompt,
      model: task.model,
      status: task.status,
      createdAt: task.createdAt,
      progressMessage: task.progressMessage,
    })),
    ...history
      .filter((turn) => !liveRunIds.has(turn.runId))
      .map((turn) => ({
        runId: turn.runId,
        prompt: turn.prompt,
        model: turn.model,
        status: turn.status,
        createdAt: turn.createdAt,
      })),
  ].sort((left, right) => right.createdAt.localeCompare(left.createdAt)), [history, liveRunIds, projectLiveTasks]);

  const selectedLiveTask = projectLiveTasks.find((task) => task.runId === selectedRunId);
  const selectedHistory = selectedLiveTask
    ? undefined
    : history.find((turn) => turn.runId === selectedRunId);
  const selectedStatus = selectedLiveTask?.status ?? selectedHistory?.status;
  const selectedAnswer = selectedLiveTask?.answer ?? selectedHistory?.response ?? "";
  const selectedError = selectedLiveTask?.error ?? selectedHistory?.error ?? "";
  const activeCount = projectLiveTasks.filter((task) => task.status === "running").length;
  const canReuseStoredKey = Boolean(
    settings?.hasApiKey
      && settings.provider === provider
      && settings.baseUrl.replace(/\/$/, "") === baseUrl.trim().replace(/\/$/, ""),
  );

  const updateTask = useCallback((runId: string, update: (task: LiveTask) => LiveTask) => {
    setLiveTasks((current) => current.map((task) => task.runId === runId ? update(task) : task));
  }, []);

  useEffect(() => {
    void getWorkflowModelSettings()
      .then((value) => {
        setSettings(value);
        if (value) {
          setProvider(value.provider);
          setBaseUrl(value.baseUrl);
          setModel(value.model);
        } else {
          setProvider(DEEPSEEK_SETTINGS.provider);
          setBaseUrl(DEEPSEEK_SETTINGS.baseUrl);
          setModel(DEEPSEEK_SETTINGS.model);
          setEditing(true);
        }
      })
      .catch((reason) => setGlobalError(normalizeCommandError(reason).message));
  }, []);

  useEffect(() => {
    let active = true;
    setPrompt("");
    setGlobalError("");
    void listWorkflowAgentTurns(project.id)
      .then((turns) => {
        if (!active) return;
        setHistory(turns);
        setSelectedRunId((current) => {
          if (projectLiveTasks.some((task) => task.runId === current)) return current;
          return projectLiveTasks.find((task) => task.status === "running")?.runId
            ?? turns.find((turn) => turn.status === "running")?.runId
            ?? turns[0]?.runId;
        });
      })
      .catch((reason) => {
        if (active) setGlobalError(normalizeCommandError(reason).message);
      });
    return () => {
      active = false;
    };
  }, [project.id]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (!focusRunId || !taskItems.some((task) => task.runId === focusRunId)) return;
    setSelectedRunId(focusRunId);
    window.requestAnimationFrame(() => cardRef.current?.scrollIntoView?.({ behavior: "smooth", block: "start" }));
  }, [focusRequest, focusRunId, taskItems]);

  useEffect(() => {
    const hasDetachedRunningTask = history.some(
      (turn) => turn.status === "running" && !liveRunIds.has(turn.runId),
    );
    if (!hasDetachedRunningTask) return;
    const timer = window.setInterval(() => {
      void refreshHistory().then(() => onTaskActivity?.()).catch(() => undefined);
    }, 3000);
    return () => window.clearInterval(timer);
  }, [history, liveRunIds, onTaskActivity, refreshHistory]);

  async function saveSettings(event: React.FormEvent) {
    event.preventDefault();
    setGlobalError("");
    try {
      const saved = await saveWorkflowModelSettings(provider, baseUrl.trim(), model.trim(), apiKey.trim());
      setSettings(saved);
      setApiKey("");
      setEditing(false);
    } catch (reason) {
      setGlobalError(normalizeCommandError(reason).message);
    }
  }

  async function removeSettings() {
    if (!window.confirm("删除本机工作流模型配置？Yuxi 问答模型不会受到影响。")) return;
    try {
      await deleteWorkflowModelSettings();
      setSettings(null);
      setProvider(DEEPSEEK_SETTINGS.provider);
      setBaseUrl(DEEPSEEK_SETTINGS.baseUrl);
      setModel(DEEPSEEK_SETTINGS.model);
      setEditing(true);
    } catch (reason) {
      setGlobalError(normalizeCommandError(reason).message);
    }
  }

  function applyDeepSeekPreset() {
    setProvider(DEEPSEEK_SETTINGS.provider);
    setBaseUrl(DEEPSEEK_SETTINGS.baseUrl);
    setModel(DEEPSEEK_SETTINGS.model);
  }

  function startNewTask() {
    setSelectedRunId(undefined);
    setPrompt("");
    setGlobalError("");
  }

  async function runAgent() {
    const taskPrompt = prompt.trim();
    if (!taskPrompt) return;
    const runId = createWorkflowRunId();
    const task: LiveTask = {
      runId,
      projectId: project.id,
      prompt: taskPrompt,
      answer: "",
      status: "running",
      model: settings?.model || model,
      createdAt: new Date().toISOString(),
      reasoning: false,
      progressMessage: "正在创建独立科研任务…",
      tools: [],
    };
    setLiveTasks((current) => [task, ...current]);
    setSelectedRunId(runId);
    setPrompt("");
    setGlobalError("");
    onTaskActivity?.();

    const channel = new Channel<WorkflowAgentEvent>();
    channel.onmessage = (event) => {
      if (event.type === "text_delta") {
        updateTask(runId, (current) => ({ ...current, reasoning: false, progressMessage: "模型正在输出结果…", answer: current.answer + event.delta }));
      } else if (event.type === "progress") {
        updateTask(runId, (current) => ({ ...current, progressMessage: event.message }));
      } else if (event.type === "reasoning_active") {
        updateTask(runId, (current) => ({ ...current, reasoning: true, progressMessage: "模型正在规划科研任务…" }));
      } else if (event.type === "tool_started") {
        const id = event.call_id || `${event.name}-${Date.now()}`;
        updateTask(runId, (current) => ({ ...current, progressMessage: `正在调用本地工具：${event.name}`, tools: [...current.tools, { id, name: event.name, preview: event.preview, status: "running" }] }));
      } else if (event.type === "tool_finished") {
        updateTask(runId, (current) => ({
          ...current,
          progressMessage: "工具执行完成，正在核验结果…",
          tools: current.tools.map((tool) => (
            tool.id === event.call_id || (!event.call_id && tool.name === event.name && tool.status === "running")
              ? { ...tool, status: event.ok ? "completed" : "failed", content: event.content }
              : tool
          )),
        }));
      } else if (event.type === "approval_required") {
        updateTask(runId, (current) => ({ ...current, progressMessage: "工作流正在等待你的操作授权…", approval: { id: event.approval_id, message: event.message } }));
      } else if (event.type === "file_changed") {
        onFilesChanged();
      } else if (event.type === "engine_error") {
        updateTask(runId, (current) => ({ ...current, error: event.message }));
      }
    };

    try {
      const completion = await runWorkflowAgent(project.id, taskPrompt, channel, runId);
      updateTask(runId, (current) => ({ ...current, answer: completion.text, status: "completed", reasoning: false, progressMessage: "已完成并保存", approval: undefined, error: undefined }));
      onFilesChanged();
    } catch (reason) {
      const message = normalizeCommandError(reason).message;
      updateTask(runId, (current) => ({ ...current, status: message.includes("取消") ? "cancelled" : "failed", reasoning: false, progressMessage: "", approval: undefined, error: message }));
    } finally {
      await refreshHistory().catch(() => undefined);
      onTaskActivity?.();
    }
  }

  async function decideApproval(approved: boolean) {
    if (!selectedLiveTask?.approval) return;
    try {
      await respondWorkflowApproval(selectedLiveTask.runId, selectedLiveTask.approval.id, approved, approved ? undefined : "用户拒绝了本次操作");
      updateTask(selectedLiveTask.runId, (current) => ({ ...current, approval: undefined, progressMessage: approved ? "授权已确认，正在继续执行…" : "已拒绝本次操作" }));
    } catch (reason) {
      updateTask(selectedLiveTask.runId, (current) => ({ ...current, error: normalizeCommandError(reason).message }));
    }
  }

  async function stopSelectedTask() {
    if (!selectedRunId || selectedStatus !== "running") return;
    try {
      const cancelled = await cancelWorkflowAgent(selectedRunId);
      if (!cancelled) {
        setGlobalError("该任务已经结束或不在当前进程中，请刷新运行记录。");
      } else if (selectedLiveTask) {
        updateTask(selectedRunId, (current) => ({ ...current, progressMessage: "正在安全停止任务…" }));
      }
    } catch (reason) {
      setGlobalError(normalizeCommandError(reason).message);
    }
  }

  if (settings === undefined) {
    return <div className="workflow-agent-loading"><LoaderCircle className="spin" />正在读取本地引擎设置…</div>;
  }

  return (
    <article className="workflow-agent-card workflow-agent-enterprise" id="workflow-agent-workbench" ref={cardRef}>
      <header>
        <div><span className="workflow-icon"><Bot size={21} /></span><div><span>LOCAL RESEARCH ORCHESTRATOR</span><h2>本地科研任务工作台</h2><p>任务级并发、独立审批与结果持久化；同一项目最多并行 3 个任务。</p></div></div>
        <div className="workflow-agent-header-actions"><span className="workflow-concurrency"><Layers3 size={14} />{activeCount}/3 运行中</span><button onClick={() => setEditing((value) => !value)}><Settings2 size={16} />模型设置</button></div>
      </header>

      {globalError && <div className="workflow-agent-error"><CircleAlert size={15} />{globalError}<button onClick={() => setGlobalError("")}><X size={13} /></button></div>}

      {editing && (
        <form className="workflow-model-form workflow-model-enterprise" onSubmit={saveSettings}>
          <div className="workflow-model-recommendation"><div><strong>DeepSeek 官方模型</strong><span>推荐 · 性价比高 · OpenAI 兼容协议</span></div><button type="button" onClick={applyDeepSeekPreset}>应用推荐配置</button></div>
          <label><span>协议</span><select value={provider} onChange={(event) => setProvider(event.target.value as WorkflowModelSettings["provider"])}><option value="openai">OpenAI Chat Completions</option><option value="openai_responses">OpenAI Responses</option><option value="anthropic">Anthropic Messages</option></select></label>
          <label><span>API Base URL</span><input value={baseUrl} onChange={(event) => setBaseUrl(event.target.value)} placeholder="https://api.deepseek.com" /></label>
          <label><span>Model</span><input value={model} onChange={(event) => setModel(event.target.value)} placeholder="deepseek-chat" /></label>
          <label><span>API Key</span><input type="password" value={apiKey} onChange={(event) => setApiKey(event.target.value)} placeholder={canReuseStoredKey ? settings?.apiKeyHint : "切换供应商或端点时必须输入新 Key"} autoComplete="off" /></label>
          <div className="workflow-model-form-actions"><button type="submit" disabled={!apiKey.trim() && !canReuseStoredKey}><Save size={14} />保存独立配置</button>{settings && <button type="button" className="danger" onClick={() => void removeSettings()}><Trash2 size={14} />删除</button>}</div>
        </form>
      )}

      {!engine?.available && <div className="workflow-agent-notice"><TerminalSquare size={17} /><div><strong>WISP Sidecar 尚未安装</strong><p>确定性 PCA 仍可运行；构建 worker 后这里会自动启用本地 Agent。</p></div></div>}

      <div className="workflow-agent-console">
        <aside className="workflow-task-rail" aria-label="科研任务">
          <div className="workflow-task-rail-header"><div><strong>科研任务</strong><span>{taskItems.length}</span></div><button type="button" onClick={startNewTask}><MessageSquarePlus size={14} />新任务</button></div>
          <div className="workflow-task-list">
            {taskItems.length === 0 && <div className="workflow-task-empty"><Clock3 size={20} /><span>尚无任务</span></div>}
            {taskItems.map((task) => (
              <button type="button" key={task.runId} className={`workflow-task-item ${selectedRunId === task.runId ? "active" : ""}`} onClick={() => setSelectedRunId(task.runId)}>
                <div><strong>{task.prompt}</strong><em className={task.status}>{statusLabel(task.status)}</em></div>
                <span>{task.model} · {new Date(task.createdAt).toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" })}</span>
                {task.status === "running" && <small><LoaderCircle className="spin" size={11} />{task.progressMessage || "正在运行"}</small>}
              </button>
            ))}
          </div>
        </aside>

        <section className="workflow-task-stage">
          {selectedRunId && <div className="workflow-task-statusbar"><div><span className={`workflow-status-dot ${selectedStatus}`} /><strong>{selectedStatus ? statusLabel(selectedStatus) : "任务"}</strong><small>{selectedLiveTask?.model || selectedHistory?.model}</small></div>{selectedStatus === "running" && <button type="button" className="stop" onClick={() => void stopSelectedTask()}><Square size={13} />停止此任务</button>}</div>}

          <div className="workflow-agent-transcript">
            {!selectedRunId && <div className="workflow-agent-welcome"><Bot size={28} /><strong>创建一个独立科研任务</strong><p>输入简单问候也会经过你配置的大模型；读取、分析和写入操作将遵循项目边界与审批策略。</p></div>}
            {selectedStatus === "running" && <div className="workflow-agent-thinking"><LoaderCircle className="spin" size={15} />{selectedLiveTask?.progressMessage || "后台任务正在运行，页面将自动刷新…"}{selectedLiveTask?.reasoning && <small>思考中</small>}</div>}
            {selectedLiveTask?.tools.length ? <div className="workflow-tool-list">{selectedLiveTask.tools.map((tool) => <details key={tool.id} className={tool.status}><summary><TerminalSquare size={14} /><span>{tool.name}</span><small>{tool.status === "running" ? "执行中" : tool.status === "completed" ? "完成" : "失败"}</small></summary><p>{tool.preview}</p>{tool.content && <pre>{tool.content}</pre>}</details>)}</div> : null}
            {selectedAnswer && <div className="workflow-agent-answer"><ReactMarkdown remarkPlugins={[remarkGfm]}>{selectedAnswer}</ReactMarkdown></div>}
            {selectedError && <div className="workflow-task-error"><CircleAlert size={16} /><span>{selectedError}</span></div>}
          </div>

          {selectedLiveTask?.approval && <div className="workflow-approval"><ShieldQuestion size={21} /><div><strong>操作需要确认</strong><p>{selectedLiveTask.approval.message}</p><span>确认前请核对命令、文件路径和影响范围。</span></div><div><button onClick={() => void decideApproval(false)}>拒绝</button><button className="approve" onClick={() => void decideApproval(true)}><CheckCircle2 size={14} />允许一次</button></div></div>}

          <div className="workflow-agent-composer"><div className="workflow-composer-label"><MessageSquarePlus size={13} /><span>提交新任务</span>{activeCount > 0 && <small>当前任务会继续在后台运行</small>}</div><textarea value={prompt} onChange={(event) => setPrompt(event.target.value)} placeholder="描述要在当前项目中完成的新科研任务…" /><button onClick={() => void runAgent()} disabled={!engine?.available || !settings?.hasApiKey || !prompt.trim() || activeCount >= 3}><Play size={15} />开始任务</button></div>
        </section>
      </div>
      <footer><KeyRound size={13} />{isDeepSeek(settings) ? "当前使用 DeepSeek 推荐配置" : "工作流凭据与 Yuxi 完全隔离"}；云端模型仅接收完成当前任务所需的项目上下文。</footer>
    </article>
  );
}
