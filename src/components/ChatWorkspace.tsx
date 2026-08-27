import { useMemo } from "react";
import {
  ActionBarPrimitive,
  AssistantRuntimeProvider,
  AuiIf,
  ComposerPrimitive,
  MessagePrimitive,
  ThreadPrimitive,
  useLocalRuntime,
  type ThreadMessageLike,
} from "@assistant-ui/react";
import { MarkdownTextPrimitive } from "@assistant-ui/react-markdown";
import { ArrowDown, ArrowUp, Check, Copy, FlaskConical, LoaderCircle, Square } from "lucide-react";
import remarkGfm from "remark-gfm";
import { createYuxiAdapter } from "../runtime/yuxi-adapter";
import type { ChatCompletion, LocalMessage } from "../types";
import { sanitizeVisibleModelText } from "../utils/reasoning-visibility";

type Props = {
  threadId: string;
  messages: LocalMessage[];
  onRunState: (state: { runId?: string; status: string; message?: string }, threadId: string) => void;
  onCompleted: (completion: ChatCompletion, threadId: string) => void;
};

function toInitialMessages(messages: LocalMessage[]): ThreadMessageLike[] {
  return messages.map((message) => ({
    id: message.id,
    role: message.role,
    content: [
      {
        type: "text",
        text: message.role === "assistant" ? sanitizeVisibleModelText(message.content) : message.content,
      },
    ],
    createdAt: new Date(message.createdAt),
  }));
}

function UserMessage() {
  return (
    <MessagePrimitive.Root className="message-row user-message-row">
      <div className="message user-message">
        <MessagePrimitive.Parts />
      </div>
    </MessagePrimitive.Root>
  );
}

function AssistantMessage() {
  return (
    <MessagePrimitive.Root className="message-row assistant-message-row">
      <div className="assistant-avatar" aria-label="稻芯智析头像">
        <img src="/brand-logo.png" alt="" />
      </div>
      <div className="assistant-message-wrap">
        <div className="assistant-name">稻芯智析</div>
        <div className="message assistant-message">
          <MessagePrimitive.Parts components={{ Text: MarkdownText }} />
        </div>
        <ActionBarPrimitive.Root className="message-actions" hideWhenRunning>
          <ActionBarPrimitive.Copy className="icon-button copy-button" copiedDuration={1600}>
            <MessagePrimitive.If copied>
              <Check size={15} />
            </MessagePrimitive.If>
            <MessagePrimitive.If copied={false}>
              <Copy size={15} />
            </MessagePrimitive.If>
            <span>复制</span>
          </ActionBarPrimitive.Copy>
        </ActionBarPrimitive.Root>
      </div>
    </MessagePrimitive.Root>
  );
}

function MarkdownText() {
  return <MarkdownTextPrimitive remarkPlugins={[remarkGfm]} />;
}

function ChatMessage() {
  return (
    <>
      <MessagePrimitive.If user>
        <UserMessage />
      </MessagePrimitive.If>
      <MessagePrimitive.If assistant>
        <AssistantMessage />
      </MessagePrimitive.If>
    </>
  );
}

function Welcome() {
  const suggestions = [
    "水稻胚乳发育分为哪些关键阶段？",
    "灌浆期淀粉合成涉及哪些核心基因？",
    "如何分析胚乳单细胞转录组数据？",
    "请解释 DAF 在水稻发育研究中的含义",
  ];
  return (
    <ThreadPrimitive.Empty>
      <section className="welcome-panel">
        <div className="welcome-mark"><img src="/brand-logo.png" alt="稻芯智析" /></div>
        <p className="eyebrow">RICE ENDOSPERM RESEARCH AGENT</p>
        <h1>你好，我是“稻芯智析”</h1>
        <p className="welcome-copy">
          面向水稻胚乳发育、灌浆调控、基因功能与组学分析的科研智能体。回答可辅助研究判断，但重要结论请结合原始文献与实验验证。
        </p>
        <div className="suggestion-grid">
          {suggestions.map((prompt) => (
            <ThreadPrimitive.Suggestion key={prompt} prompt={prompt} send className="suggestion-card">
              <FlaskConical size={18} />
              <span>{prompt}</span>
            </ThreadPrimitive.Suggestion>
          ))}
        </div>
      </section>
    </ThreadPrimitive.Empty>
  );
}

function Composer() {
  return (
    <div className="composer-shell">
      <ComposerPrimitive.Root className="composer">
        <ComposerPrimitive.Input
          className="composer-input"
          placeholder="询问水稻胚乳发育、基因调控或组学分析问题……"
          rows={1}
          aria-label="输入问题"
        />
        <AuiIf condition={(state) => !state.thread.isRunning}>
          <ComposerPrimitive.Send className="send-button" aria-label="发送">
            <ArrowUp size={20} strokeWidth={2.4} />
          </ComposerPrimitive.Send>
        </AuiIf>
        <AuiIf condition={(state) => state.thread.isRunning}>
          <ComposerPrimitive.Cancel className="send-button cancel-button" aria-label="停止生成">
            <Square size={16} fill="currentColor" />
          </ComposerPrimitive.Cancel>
        </AuiIf>
      </ComposerPrimitive.Root>
      <p className="composer-note">AI 生成内容可能存在偏差，科研结论请核对原始证据。</p>
    </div>
  );
}

function RuntimeThread({ threadId, messages, onRunState, onCompleted }: Props) {
  const adapter = useMemo(
    () =>
      createYuxiAdapter(threadId, {
        // 附带本组件的 threadId，让上层能区分完成事件来自哪个会话
        //（切换会话后旧 run 的迟到回调不应污染新会话状态）。
        onRunState: (state) => onRunState(state, threadId),
        onCompleted: (completion) => onCompleted(completion, threadId),
      }),
    [threadId, onRunState, onCompleted],
  );
  const runtime = useLocalRuntime(adapter, {
    initialMessages: toInitialMessages(messages),
  });

  return (
    <AssistantRuntimeProvider runtime={runtime}>
      <ThreadPrimitive.Root className="thread-root">
        <ThreadPrimitive.Viewport className="thread-viewport">
          <Welcome />
          <ThreadPrimitive.Messages components={{ Message: ChatMessage }} />
          <AuiIf condition={(state) => state.thread.isRunning}>
            <div className="thinking-status" role="status" aria-live="polite">
              <LoaderCircle size={16} />
              <span>思考中…</span>
            </div>
          </AuiIf>
          <ThreadPrimitive.ScrollToBottom className="scroll-bottom" aria-label="滚动到底部">
            <ArrowDown size={18} />
          </ThreadPrimitive.ScrollToBottom>
          <ThreadPrimitive.ViewportFooter className="thread-footer">
            <Composer />
          </ThreadPrimitive.ViewportFooter>
        </ThreadPrimitive.Viewport>
      </ThreadPrimitive.Root>
    </AssistantRuntimeProvider>
  );
}

export function ChatWorkspace(props: Props) {
  return <RuntimeThread key={props.threadId} {...props} />;
}
