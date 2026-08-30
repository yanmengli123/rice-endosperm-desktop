import { Channel } from "@tauri-apps/api/core";
import {
  Activity,
  BarChart3,
  Bot,
  CheckCircle2,
  ChevronRight,
  CircleAlert,
  Clock3,
  Cpu,
  FileChartColumn,
  FileText,
  FolderOpen,
  FolderPlus,
  HardDrive,
  LoaderCircle,
  MessageSquareText,
  Play,
  ShieldCheck,
  Square,
  Trash2,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import {
  cancelWorkflowRun,
  createWorkflowProject,
  deleteWorkflowProject,
  getWorkflowEngineStatus,
  listWorkflowArtifacts,
  listWorkflowProjects,
  listWorkflowRuns,
  normalizeCommandError,
  openWorkflowArtifact,
  pickWorkflowDirectory,
  runCountsPcaWorkflow,
} from "../services/tauri-client";
import type {
  WorkflowArtifact,
  WorkflowEngineStatus,
  WorkflowEvent,
  WorkflowProject,
  WorkflowRun,
} from "../types";
import { WorkflowAgentPanel } from "./WorkflowAgentPanel";

type Props = {
  onOpenQa: () => void;
  qaAvailable: boolean;
};

function formatBytes(value: number) {
  if (value < 1024) return `${value} B`;
  if (value < 1024 ** 2) return `${(value / 1024).toFixed(1)} KB`;
  return `${(value / 1024 ** 2).toFixed(1)} MB`;
}

function statusLabel(status: WorkflowRun["status"]) {
  return {
    queued: "排队中",
    running: "运行中",
    completed: "已完成",
    failed: "失败",
    cancelled: "已取消",
    interrupted: "已中断",
  }[status];
}

export function WorkflowWorkspace({ onOpenQa, qaAvailable }: Props) {
  const [projects, setProjects] = useState<WorkflowProject[]>([]);
  const [activeProjectId, setActiveProjectId] = useState<string>();
  const [runs, setRuns] = useState<WorkflowRun[]>([]);
  const [artifacts, setArtifacts] = useState<WorkflowArtifact[]>([]);
  const [engine, setEngine] = useState<WorkflowEngineStatus>();
  const [inputPath, setInputPath] = useState("input/counts.csv");
  const [busy, setBusy] = useState(false);
  const [activeRunId, setActiveRunId] = useState<string>();
  const [progress, setProgress] = useState({ percent: 0, message: "" });
  const [error, setError] = useState("");
  const [workspaceTool, setWorkspaceTool] = useState<"agent" | "pca">("agent");
  const activeProject = useMemo(
    () => projects.find((project) => project.id === activeProjectId),
    [activeProjectId, projects],
  );

  const refreshProjects = useCallback(async () => {
    const next = await listWorkflowProjects();
    setProjects(next);
    setActiveProjectId((current) => current && next.some((project) => project.id === current)
      ? current
      : next[0]?.id);
  }, []);

  const refreshProjectData = useCallback(async (projectId: string) => {
    const [nextRuns, nextArtifacts] = await Promise.all([
      listWorkflowRuns(projectId),
      listWorkflowArtifacts(projectId),
    ]);
    setRuns(nextRuns);
    setArtifacts(nextArtifacts);
  }, []);

  useEffect(() => {
    void Promise.all([refreshProjects(), getWorkflowEngineStatus().then(setEngine)])
      .catch((reason) => setError(normalizeCommandError(reason).message));
  }, [refreshProjects]);

  useEffect(() => {
    if (!activeProjectId) {
      setRuns([]);
      setArtifacts([]);
      return;
    }
    void refreshProjectData(activeProjectId)
      .catch((reason) => setError(normalizeCommandError(reason).message));
  }, [activeProjectId, refreshProjectData]);

  async function addProject() {
    setError("");
    try {
      const root = await pickWorkflowDirectory();
      if (!root) return;
      const fallback = root.split(/[\\/]/).filter(Boolean).slice(-1)[0] || "科研项目";
      const name = window.prompt("输入科研项目名称", fallback)?.trim();
      if (!name) return;
      const project = await createWorkflowProject(root, name);
      await refreshProjects();
      setActiveProjectId(project.id);
    } catch (reason) {
      setError(normalizeCommandError(reason).message);
    }
  }

  async function removeProject(project: WorkflowProject) {
    if (!window.confirm(`从工作台移除“${project.name}”？项目文件不会被删除。`)) return;
    try {
      await deleteWorkflowProject(project.id);
      await refreshProjects();
    } catch (reason) {
      setError(normalizeCommandError(reason).message);
    }
  }

  async function runPca() {
    if (!activeProject) return;
    setBusy(true);
    setError("");
    setProgress({ percent: 5, message: "正在创建运行" });
    const channel = new Channel<WorkflowEvent>();
    channel.onmessage = (event) => {
      if (event.type === "run_started") {
        setActiveRunId(event.run_id);
        setProgress({ percent: 10, message: event.message });
      } else if (event.type === "progress") {
        setProgress({ percent: event.percent, message: event.message });
      } else if (event.type === "run_failed") {
        setError(event.message);
      }
    };
    try {
      const run = await runCountsPcaWorkflow(activeProject.id, inputPath.trim(), channel);
      if (run.status === "failed") setError(run.error || "PCA 工作流失败");
      await refreshProjectData(activeProject.id);
    } catch (reason) {
      setError(normalizeCommandError(reason).message);
    } finally {
      setBusy(false);
      setActiveRunId(undefined);
      setProgress({ percent: 0, message: "" });
    }
  }

  async function cancelPca() {
    if (activeRunId) await cancelWorkflowRun(activeRunId);
  }

  return (
    <div className="workflow-shell">
      <aside className="workflow-sidebar">
        <div className="sidebar-brand">
          <img src="/brand-logo.png" alt="" />
          <div><strong>稻芯智析</strong><span>科研工作流</span></div>
        </div>
        <div className="product-mode-switch" aria-label="产品模块">
          <button onClick={onOpenQa}><MessageSquareText size={16} />智能问答</button>
          <button className="active"><Cpu size={16} />科研工作流</button>
        </div>
        <button className="new-thread-button" onClick={() => void addProject()}>
          <FolderPlus size={18} /> 新建本地项目
        </button>
        <div className="thread-section-title">科研项目</div>
        <nav className="workflow-project-list">
          {projects.map((project) => (
            <div key={project.id} className={`workflow-project-item ${project.id === activeProjectId ? "active" : ""}`}>
              <button onClick={() => setActiveProjectId(project.id)}>
                <FolderOpen size={17} />
                <span><strong>{project.name}</strong><small>{project.root}</small></span>
              </button>
              <button className="workflow-project-delete" onClick={() => void removeProject(project)} aria-label="移除项目"><Trash2 size={14} /></button>
            </div>
          ))}
        </nav>
        <div className="workflow-engine-card">
          <span className={engine?.available ? "ready" : "local"}><Activity size={15} />{engine?.available ? "WISP 已就绪" : "本地执行器已就绪"}</span>
          <small>{engine?.protocol || "rice.workflow.v1"}</small>
        </div>
      </aside>

      <main className="workflow-main">
        <header className="workflow-topbar">
          <div><span>LOCAL SCIENCE WORKBENCH</span><h1>{activeProject?.name || "科研工作流"}</h1></div>
          <div className="workflow-engine-status"><ShieldCheck size={16} /><span><strong>项目隔离</strong><small>{engine?.message || "正在检查本地引擎"}</small></span></div>
        </header>
        {error && <div className="global-error"><CircleAlert size={17} />{error}<button onClick={() => setError("")}>关闭</button></div>}

        {!activeProject ? (
          <section className="workflow-empty">
            <div><FolderPlus size={34} /></div>
            <h2>创建第一个本地科研项目</h2>
            <p>项目数据保留在你选择的文件夹中，不依赖 Yuxi 服务端。系统会建立 input、work、results、reports 和运行审计目录。</p>
            <button onClick={() => void addProject()}><FolderOpen size={17} />选择项目文件夹</button>
          </section>
        ) : (
          <div className="workflow-dashboard">
            <section className="workflow-center">
              <div className="workflow-tool-tabs">
                <button className={workspaceTool === "agent" ? "active" : ""} onClick={() => setWorkspaceTool("agent")}><Bot size={15} />本地科研助手</button>
                <button className={workspaceTool === "pca" ? "active" : ""} onClick={() => setWorkspaceTool("pca")}><BarChart3 size={15} />确定性 PCA</button>
              </div>
              {workspaceTool === "agent" ? (
                <WorkflowAgentPanel
                  project={activeProject}
                  engine={engine}
                  onFilesChanged={() => void refreshProjectData(activeProject.id)}
                />
              ) : (
              <article className="workflow-hero-card">
                <div className="workflow-card-heading">
                  <span className="workflow-icon"><BarChart3 size={21} /></span>
                  <div><span>DETERMINISTIC PIPELINE</span><h2>表达矩阵主成分分析</h2><p>读取非负 counts 矩阵，执行 log2(count + 1)、样本 PCA、科研图形与可复现清单。</p></div>
                </div>
                <div className="workflow-path"><HardDrive size={15} /><span>{activeProject.root}</span></div>
                <label className="workflow-input-label">
                  <span>项目内输入路径</span>
                  <input value={inputPath} onChange={(event) => setInputPath(event.target.value)} disabled={busy} />
                  <small>文件必须位于 input/ 内，支持 CSV、TSV 和制表符 TXT；第一列为基因标识，其余列为样本。</small>
                </label>
                {busy && (
                  <div className="workflow-progress">
                    <div><span style={{ width: `${progress.percent}%` }} /></div>
                    <p><LoaderCircle className="spin" size={15} />{progress.message}</p>
                  </div>
                )}
                <div className="workflow-actions">
                  {busy ? (
                    <button className="stop" onClick={() => void cancelPca()}><Square size={15} />停止运行</button>
                  ) : (
                    <button onClick={() => void runPca()}><Play size={15} />运行 PCA 工作流</button>
                  )}
                </div>
              </article>
              )}

              <article className="workflow-policy-card">
                <ShieldCheck size={22} />
                <div><strong>数据边界已启用</strong><p>受控工作流只读取 input/，结果写入 results/ 与 reports/；输入校验和、算法参数和输出校验和写入 workflow-manifest.json。</p></div>
              </article>
            </section>

            <aside className="workflow-inspector">
              <section>
                <header><div><Clock3 size={16} /><strong>运行记录</strong></div><span>{runs.length}</span></header>
                <div className="workflow-record-list">
                  {runs.length === 0 && <p className="workflow-placeholder">尚无运行记录</p>}
                  {runs.map((run) => (
                    <article key={run.id} className={`workflow-run ${run.status}`}>
                      <div><strong>{run.workflowKind}</strong><span>{statusLabel(run.status)}</span></div>
                      <small>{new Date(run.createdAt).toLocaleString("zh-CN")}</small>
                      {run.error && <p>{run.error}</p>}
                    </article>
                  ))}
                </div>
              </section>
              <section>
                <header><div><FileChartColumn size={16} /><strong>科研产物</strong></div><span>{artifacts.length}</span></header>
                <div className="workflow-record-list">
                  {artifacts.length === 0 && <p className="workflow-placeholder">运行完成后在这里显示结果</p>}
                  {artifacts.map((artifact) => (
                    <button className="workflow-artifact" key={artifact.id} onClick={() => void openWorkflowArtifact(artifact.id)}>
                      {artifact.mediaType.startsWith("image/") ? <BarChart3 size={18} /> : <FileText size={18} />}
                      <span><strong>{artifact.name}</strong><small>{formatBytes(artifact.sizeBytes)} · {artifact.sha256.slice(0, 12)}…</small></span>
                      <ChevronRight size={15} />
                    </button>
                  ))}
                </div>
              </section>
              <section className="workflow-bridge-card">
                <header><div><Bot size={16} /><strong>Artifact Bridge</strong></div></header>
                <p>成果只有在你明确选择后才会发送到科研问答，不共享会话、记忆或数据库。</p>
                <button disabled={!qaAvailable || artifacts.length === 0} onClick={onOpenQa}>
                  <CheckCircle2 size={15} />进入问答并选择成果
                </button>
              </section>
            </aside>
          </div>
        )}
      </main>
    </div>
  );
}
