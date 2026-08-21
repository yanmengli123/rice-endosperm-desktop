import { useCallback, useEffect, useRef, useState } from "react";
import { CircleAlert, LoaderCircle, PanelLeft, Wifi, WifiOff } from "lucide-react";
import { ChatWorkspace } from "./components/ChatWorkspace";
import { ConnectionSetup } from "./components/ConnectionSetup";
import { SettingsDialog } from "./components/SettingsDialog";
import { Sidebar } from "./components/Sidebar";
import { RunContextBar } from "./components/RunContextBar";
import {
  createThread,
  deleteThread,
  getPublicSettings,
  getThreadRunContext,
  listThreads,
  loadMessages,
  normalizeCommandError,
  renameThread,
  syncPendingRuns,
} from "./services/tauri-client";
import type { ChatCompletion, LocalMessage, PublicSettings, ServerRunContext, ThreadSummary } from "./types";
import "./styles.css";

const FALLBACK_GATEWAY = "http://127.0.0.1:9088";

export default function App() {
  const [settings, setSettings] = useState<PublicSettings>();
  const [threads, setThreads] = useState<ThreadSummary[]>([]);
  const [activeThreadId, setActiveThreadId] = useState<string>();
  const [messages, setMessages] = useState<LocalMessage[]>([]);
  const [runContext, setRunContext] = useState<ServerRunContext | null>(null);
  const [recoveryPending, setRecoveryPending] = useState(0);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [runState, setRunState] = useState<{ status: string; message?: string }>({ status: "idle" });
  const initialized = useRef(false);
  const threadLoadSequence = useRef(0);

  const refreshThreads = useCallback(async () => {
    const items = await listThreads();
    setThreads(items);
    return items;
  }, []);

  const openThread = useCallback(async (threadId: string) => {
    const sequence = ++threadLoadSequence.current;
    setActiveThreadId(threadId);
    const [threadMessages, context] = await Promise.all([
      loadMessages(threadId),
      getThreadRunContext(threadId),
    ]);
    if (sequence !== threadLoadSequence.current) return;
    setMessages(threadMessages);
    setRunContext(context);
  }, []);

  const ensureThread = useCallback(async () => {
    const items = await refreshThreads();
    const first = items[0] ?? (await createThread());
    if (items.length === 0) setThreads([first]);
    await openThread(first.id);
    return first.id;
  }, [openThread, refreshThreads]);

  useEffect(() => {
    if (initialized.current) return;
    initialized.current = true;
    void (async () => {
      try {
        const current = await getPublicSettings();
        setSettings(current);
        if (current.hasApiKey) {
          const threadId = await ensureThread();
          const recovery = await syncPendingRuns();
          setRecoveryPending(recovery.pending);
          if (recovery.recovered > 0 || recovery.failed > 0) {
            await refreshThreads();
            await openThread(threadId);
          }
          if (recovery.failed > 0 && recovery.lastError) setError(recovery.lastError);
        }
      } catch (reason) {
        setError(normalizeCommandError(reason).message);
      } finally {
        setLoading(false);
      }
    })();
  }, [ensureThread, openThread, refreshThreads]);

  useEffect(() => {
    if (!settings?.hasApiKey || recoveryPending === 0) return;
    const timeout = window.setTimeout(() => {
      void syncPendingRuns()
        .then(async (recovery) => {
          setRecoveryPending(recovery.pending);
          if (recovery.recovered > 0 || recovery.failed > 0) {
            await refreshThreads();
            if (activeThreadId) await openThread(activeThreadId);
          }
          if (recovery.failed > 0 && recovery.lastError) setError(recovery.lastError);
        })
        .catch((reason) => {
          setError(normalizeCommandError(reason).message);
          setRecoveryPending(0);
        });
    }, 4000);
    return () => window.clearTimeout(timeout);
  }, [activeThreadId, openThread, recoveryPending, refreshThreads, settings?.hasApiKey]);

  async function newThread() {
    try {
      const thread = await createThread();
      setThreads((current) => [thread, ...current]);
      await openThread(thread.id);
    } catch (reason) {
      setError(normalizeCommandError(reason).message);
    }
  }

  async function removeThread(threadId: string) {
    if (!window.confirm("确定删除这段本地研究对话吗？此操作无法撤销。")) return;
    try {
      await deleteThread(threadId);
      const remaining = await refreshThreads();
      if (threadId === activeThreadId) {
        const next = remaining[0] ?? (await createThread());
        if (remaining.length === 0) setThreads([next]);
        await openThread(next.id);
      }
    } catch (reason) {
      setError(normalizeCommandError(reason).message);
    }
  }

  async function changeTitle(threadId: string, currentTitle: string) {
    const title = window.prompt("输入新的会话标题", currentTitle)?.trim();
    if (!title || title === currentTitle) return;
    try {
      await renameThread(threadId, title);
      await refreshThreads();
    } catch (reason) {
      setError(normalizeCommandError(reason).message);
    }
  }

  const completed = useCallback((completion: ChatCompletion) => {
    setRunContext(completion.context);
    void refreshThreads();
  }, [refreshThreads]);

  if (loading) {
    return <main className="boot-screen"><img src="/brand-logo.png" alt="" /><LoaderCircle className="spin" /><p>正在启动稻芯智析…</p></main>;
  }

  if (!settings?.hasApiKey) {
    return (
      <ConnectionSetup
        defaultGatewayUrl={settings?.gatewayUrl || FALLBACK_GATEWAY}
        onConnected={(connected) => {
          setSettings(connected);
          setLoading(true);
          void ensureThread()
            .then(async (threadId) => {
              const recovery = await syncPendingRuns();
              setRecoveryPending(recovery.pending);
              if (recovery.recovered > 0 || recovery.failed > 0) {
                await refreshThreads();
                await openThread(threadId);
              }
              if (recovery.failed > 0 && recovery.lastError) setError(recovery.lastError);
            })
            .catch((reason) => setError(normalizeCommandError(reason).message))
            .finally(() => setLoading(false));
        }}
      />
    );
  }

  return (
    <div className={`app-shell ${sidebarOpen ? "sidebar-visible" : ""}`}>
      {sidebarOpen && (
        <Sidebar
          threads={threads}
          activeThreadId={activeThreadId}
          onSelect={(id) => void openThread(id)}
          onNew={() => void newThread()}
          onDelete={(id) => void removeThread(id)}
          onRename={(id, title) => void changeTitle(id, title)}
          onSettings={() => setSettingsOpen(true)}
        />
      )}
      <main className="main-panel">
        <header className="topbar">
          <div className="topbar-left">
            <button className="icon-button" onClick={() => setSidebarOpen((value) => !value)} aria-label="展开或收起侧栏"><PanelLeft size={20} /></button>
            <div><strong>{threads.find((thread) => thread.id === activeThreadId)?.title || "新对话"}</strong><span>稻芯智析 · 水稻胚乳科研智能体</span></div>
          </div>
          <div className={`connection-badge ${runState.status === "failed" ? "error" : ""}`}>
            {runState.status === "failed" ? <WifiOff size={15} /> : <Wifi size={15} />}
            {runState.message || (runState.status === "running" ? "正在分析" : "服务已连接")}
          </div>
        </header>
        <RunContextBar context={runContext} />
        {error && <div className="global-error"><CircleAlert size={17} />{error}<button onClick={() => setError("")}>关闭</button></div>}
        {activeThreadId ? (
          <ChatWorkspace
            key={`${activeThreadId}-${messages.length}`}
            threadId={activeThreadId}
            messages={messages}
            onRunState={setRunState}
            onCompleted={completed}
          />
        ) : (
          <div className="panel-loading"><LoaderCircle className="spin" />正在载入会话…</div>
        )}
      </main>
      {settingsOpen && (
        <SettingsDialog
          settings={settings}
          onClose={() => setSettingsOpen(false)}
          onCredentialDeleted={() => {
            setSettingsOpen(false);
            setSettings({ ...settings, hasApiKey: false, apiKeyHint: undefined });
          }}
        />
      )}
    </div>
  );
}
