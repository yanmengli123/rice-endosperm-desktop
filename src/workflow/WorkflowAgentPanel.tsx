import { Channel } from "@tauri-apps/api/core";
import {
  Bot,
  CheckCircle2,
  CircleAlert,
  KeyRound,
  LoaderCircle,
  Play,
  Save,
  Settings2,
  ShieldQuestion,
  Square,
  TerminalSquare,
  Trash2,
  X,
} from "lucide-react";
import { useCallback, useEffect, useState } from "react";
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
  onFilesChanged: () => void;
};

type ToolActivity = {
  id: string;
  name: string;
  preview: string;
  status: "running" | "completed" | "failed";
  content?: string;
};

type Approval = { id: string; message: string };

const DEFAULT_SETTINGS: WorkflowModelSettings = {
  provider: "openai",
  baseUrl: "https://api.deepseek.com",
  model: "deepseek-chat",
  hasApiKey: false,
};

export function WorkflowAgentPanel({ project, engine, onFilesChanged }: Props) {
  const [settings, setSettings] = useState<WorkflowModelSettings | null>();
  const [editing, setEditing] = useState(false);
  const [provider, setProvider] = useState<WorkflowModelSettings["provider"]>("openai");
  const [baseUrl, setBaseUrl] = useState(DEFAULT_SETTINGS.baseUrl);
  const [model, setModel] = useState(DEFAULT_SETTINGS.model);
  const [apiKey, setApiKey] = useState("");
  const [prompt, setPrompt] = useState("");
  const [answer, setAnswer] = useState("");
  const [running, setRunning] = useState(false);
  const [reasoning, setReasoning] = useState(false);
  const [progressMessage, setProgressMessage] = useState("");
  const [tools, setTools] = useState<ToolActivity[]>([]);
  const [approval, setApproval] = useState<Approval>();
  const [error, setError] = useState("");
  const [history, setHistory] = useState<WorkflowAgentTurn[]>([]);
  const [selectedTurnId, setSelectedTurnId] = useState<string>();

  const refreshHistory = useCallback(async () => {
    const turns = await listWorkflowAgentTurns(project.id);
    setHistory(turns);
    return turns;
  }, [project.id]);

  useEffect(() => {
    void getWorkflowModelSettings()
      .then((value) => {
        setSettings(value);
        if (value) {
          setProvider(value.provider);
          setBaseUrl(value.baseUrl);
          setModel(value.model);
        } else {
          setEditing(true);
        }
      })
      .catch((reason) => setError(normalizeCommandError(reason).message));
  }, []);

  useEffect(() => {
    let active = true;
    setAnswer("");
    setTools([]);
    setApproval(undefined);
    setSelectedTurnId(undefined);
    setError("");
    void listWorkflowAgentTurns(project.id)
      .then((turns) => {
        if (!active) return;
        setHistory(turns);
        const latest = turns.find((turn) => Boolean(turn.response));
        if (latest) {
          setSelectedTurnId(latest.id);
          setAnswer(latest.response);
          setError(latest.error || "");
        }
      })
      .catch((reason) => {
        if (active) setError(normalizeCommandError(reason).message);
      });
    return () => {
      active = false;
    };
  }, [project.id]);

  async function saveSettings(event: React.FormEvent) {
    event.preventDefault();
    setError("");
    try {
      const saved = await saveWorkflowModelSettings(
        provider,
        baseUrl.trim(),
        model.trim(),
        apiKey.trim(),
      );
      setSettings(saved);
      setApiKey("");
      setEditing(false);
    } catch (reason) {
      setError(normalizeCommandError(reason).message);
    }
  }

  async function removeSettings() {
    if (!window.confirm("删除本机工作流模型配置？Yuxi 问答模型不会受到影响。")) return;
    try {
      await deleteWorkflowModelSettings();
      setSettings(null);
      setEditing(true);
    } catch (reason) {
      setError(normalizeCommandError(reason).message);
    }
  }

  async function runAgent() {
    if (!prompt.trim() || running) return;
    setRunning(true);
    setReasoning(false);
    setProgressMessage("正在启动并校验本地科研引擎…");
    setAnswer("");
    setTools([]);
    setError("");
    const channel = new Channel<WorkflowAgentEvent>();
    channel.onmessage = (event) => {
      if (event.type === "text_delta") {
        setReasoning(false);
        setProgressMessage("模型正在输出结果…");
        setAnswer((current) => current + event.delta);
      } else if (event.type === "progress") {
        setProgressMessage(event.message);
      } else if (event.type === "reasoning_active") {
        setReasoning(true);
        setProgressMessage("模型正在规划科研任务…");
      } else if (event.type === "tool_started") {
        setProgressMessage(`正在调用本地工具：${event.name}`);
        const id = event.call_id || `${event.name}-${Date.now()}`;
        setTools((current) => [...current, { id, name: event.name, preview: event.preview, status: "running" }]);
      } else if (event.type === "tool_finished") {
        setProgressMessage("工具执行完成，正在核验结果…");
        setTools((current) => current.map((tool) => (
          tool.id === event.call_id || (!event.call_id && tool.name === event.name && tool.status === "running")
            ? { ...tool, status: event.ok ? "completed" : "failed", content: event.content }
            : tool
        )));
      } else if (event.type === "approval_required") {
        setProgressMessage("工作流正在等待你的操作授权…");
        setApproval({ id: event.approval_id, message: event.message });
      } else if (event.type === "file_changed") {
        onFilesChanged();
      } else if (event.type === "engine_error") {
        setError(event.message);
      } else if (event.type === "turn_completed") {
        setProgressMessage("");
      }
    };
    try {
      const completion = await runWorkflowAgent(project.id, prompt.trim(), channel);
      setAnswer(completion.text);
      setPrompt("");
      onFilesChanged();
      const turns = await refreshHistory();
      setSelectedTurnId(turns[0]?.id);
    } catch (reason) {
      setError(normalizeCommandError(reason).message);
    } finally {
      setRunning(false);
      setReasoning(false);
      setProgressMessage("");
      setApproval(undefined);
    }
  }

  async function decideApproval(approved: boolean) {
    if (!approval) return;
    try {
      await respondWorkflowApproval(project.id, approval.id, approved, approved ? undefined : "用户拒绝了本次操作");
      setApproval(undefined);
    } catch (reason) {
      setError(normalizeCommandError(reason).message);
    }
  }

  if (settings === undefined) {
    return <div className="workflow-agent-loading"><LoaderCircle className="spin" />正在读取本地引擎设置…</div>;
  }

  return (
    <article className="workflow-agent-card">
      <header>
        <div><span className="workflow-icon"><Bot size={21} /></span><div><span>WISP LOCAL AGENT</span><h2>本地科研计算助手</h2><p>只在当前项目根目录内工作；读取路径受限，所有修改型工具都必须审批。</p></div></div>
        <button onClick={() => setEditing((value) => !value)}><Settings2 size={16} />模型设置</button>
      </header>

      {error && <div className="workflow-agent-error"><CircleAlert size={15} />{error}<button onClick={() => setError("")}><X size={13} /></button></div>}

      {editing && (
        <form className="workflow-model-form" onSubmit={saveSettings}>
          <label><span>协议</span><select value={provider} onChange={(event) => setProvider(event.target.value as WorkflowModelSettings["provider"])}><option value="openai">OpenAI Chat Completions</option><option value="openai_responses">OpenAI Responses</option><option value="anthropic">Anthropic Messages</option></select></label>
          <label><span>API Base URL</span><input value={baseUrl} onChange={(event) => setBaseUrl(event.target.value)} placeholder="https://api.example.com/v1" /></label>
          <label><span>Model</span><input value={model} onChange={(event) => setModel(event.target.value)} placeholder="provider/model" /></label>
          <label><span>API Key</span><input type="password" value={apiKey} onChange={(event) => setApiKey(event.target.value)} placeholder={settings?.apiKeyHint || "仅保存在 Stronghold"} autoComplete="off" /></label>
          <div><button type="submit" disabled={!apiKey.trim() && !settings?.hasApiKey}><Save size={14} />保存独立配置</button>{settings && <button type="button" className="danger" onClick={() => void removeSettings()}><Trash2 size={14} />删除</button>}</div>
        </form>
      )}

      {!engine?.available && <div className="workflow-agent-notice"><TerminalSquare size={17} /><div><strong>WISP Sidecar 尚未安装</strong><p>确定性 PCA 仍可运行；构建 worker 后这里会自动启用本地 Agent。</p></div></div>}

      <div className="workflow-agent-transcript">
        {!answer && !running && <div className="workflow-agent-welcome"><Bot size={26} /><strong>让本地计算引擎处理科研文件</strong><p>例如：检查 input 目录中的 counts.csv，给出分析计划。执行写文件、Shell、Python 或 R 前会弹出审批。</p></div>}
        {running && progressMessage && <div className="workflow-agent-thinking"><LoaderCircle className="spin" size={15} />{progressMessage}{reasoning && <small>思考中</small>}</div>}
        {tools.length > 0 && <div className="workflow-tool-list">{tools.map((tool) => <details key={tool.id} className={tool.status}><summary><TerminalSquare size={14} /><span>{tool.name}</span><small>{tool.status === "running" ? "执行中" : tool.status === "completed" ? "完成" : "失败"}</small></summary><p>{tool.preview}</p>{tool.content && <pre>{tool.content}</pre>}</details>)}</div>}
        {answer && <div className="workflow-agent-answer"><ReactMarkdown remarkPlugins={[remarkGfm]}>{answer}</ReactMarkdown></div>}
      </div>

      {history.length > 0 && (
        <section className="workflow-agent-history">
          <header><strong>已持久化回合</strong><span>{history.length}</span></header>
          <div>
            {history.map((turn) => (
              <button
                key={turn.id}
                className={selectedTurnId === turn.id ? "active" : ""}
                onClick={() => {
                  setSelectedTurnId(turn.id);
                  setAnswer(turn.response);
                  setTools([]);
                  setError(turn.error || "");
                }}
              >
                <span><strong>{turn.prompt}</strong><small>{turn.model} · {new Date(turn.createdAt).toLocaleString("zh-CN")}</small></span>
                <em className={turn.status}>{turn.status === "completed" ? "完成" : turn.status === "running" ? "运行中" : turn.status === "cancelled" ? "已取消" : turn.status === "interrupted" ? "已中断" : "失败"}</em>
              </button>
            ))}
          </div>
        </section>
      )}

      {approval && <div className="workflow-approval"><ShieldQuestion size={21} /><div><strong>操作需要确认</strong><p>{approval.message}</p><span>确认前请核对命令、文件路径和影响范围。</span></div><div><button onClick={() => void decideApproval(false)}>拒绝</button><button className="approve" onClick={() => void decideApproval(true)}><CheckCircle2 size={14} />允许一次</button></div></div>}

      <div className="workflow-agent-composer">
        <textarea value={prompt} onChange={(event) => setPrompt(event.target.value)} placeholder="描述要在当前项目中完成的科研计算任务…" disabled={running} />
        {running ? <button className="stop" onClick={() => void cancelWorkflowAgent(project.id)}><Square size={15} />停止</button> : <button onClick={() => void runAgent()} disabled={!engine?.available || !settings?.hasApiKey || !prompt.trim()}><Play size={15} />执行</button>}
      </div>
      <footer><KeyRound size={13} />工作流凭据与 Yuxi 完全隔离；使用云端模型时，必要项目上下文可能发送给该模型供应商。</footer>
    </article>
  );
}
