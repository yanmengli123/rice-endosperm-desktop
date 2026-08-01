import { MessageSquareText, MoreHorizontal, Plus, Settings2, Trash2 } from "lucide-react";
import type { ThreadSummary } from "../types";
import packageInfo from "../../package.json";

type Props = {
  threads: ThreadSummary[];
  activeThreadId?: string;
  onSelect: (threadId: string) => void;
  onNew: () => void;
  onDelete: (threadId: string) => void;
  onRename: (threadId: string, currentTitle: string) => void;
  onSettings: () => void;
};

function relativeTime(value: string) {
  const time = new Date(value).getTime();
  const difference = Date.now() - time;
  if (difference < 60_000) return "刚刚";
  if (difference < 3_600_000) return `${Math.floor(difference / 60_000)} 分钟前`;
  if (difference < 86_400_000) return `${Math.floor(difference / 3_600_000)} 小时前`;
  return new Intl.DateTimeFormat("zh-CN", { month: "numeric", day: "numeric" }).format(time);
}

export function Sidebar({
  threads,
  activeThreadId,
  onSelect,
  onNew,
  onDelete,
  onRename,
  onSettings,
}: Props) {
  return (
    <aside className="sidebar">
      <div className="sidebar-brand">
        <img src="/brand-logo.png" alt="" />
        <div><strong>稻芯智析</strong><span>科研智能体</span></div>
      </div>
      <button className="new-thread-button" onClick={onNew}>
        <Plus size={18} /> 新建对话
      </button>
      <div className="thread-section-title">研究记录</div>
      <nav className="thread-list" aria-label="历史会话">
        {threads.map((thread) => (
          <div
            key={thread.id}
            className={`thread-item ${thread.id === activeThreadId ? "active" : ""}`}
          >
            <button className="thread-main" onClick={() => onSelect(thread.id)}>
              <MessageSquareText size={17} />
              <span className="thread-copy">
                <strong>{thread.title}</strong>
                <small>{thread.preview || relativeTime(thread.updatedAt)}</small>
              </span>
            </button>
            <div className="thread-actions">
              <button onClick={() => onRename(thread.id, thread.title)} title="重命名"><MoreHorizontal size={15} /></button>
              <button onClick={() => onDelete(thread.id)} title="删除"><Trash2 size={15} /></button>
            </div>
          </div>
        ))}
      </nav>
      <div className="sidebar-footer">
        <button onClick={onSettings}><Settings2 size={18} /> 设置与连接</button>
        <span>v{packageInfo.version}</span>
      </div>
    </aside>
  );
}
