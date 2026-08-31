import { useMemo } from "react";
import {
  ActionBarPrimitive,
  AssistantRuntimeProvider,
  AttachmentPrimitive,
  AuiIf,
  ComposerPrimitive,
  MessagePrimitive,
  ThreadPrimitive,
  useLocalRuntime,
  type ThreadMessageLike,
} from "@assistant-ui/react";
import { MarkdownTextPrimitive } from "@assistant-ui/react-markdown";
import { ArrowDown, ArrowUp, Check, Copy, FlaskConical, LoaderCircle, Paperclip, Square, X } from "lucide-react";
import rehypeRaw from "rehype-raw";
import rehypeKatex from "rehype-katex";
import rehypeSanitize, { defaultSchema } from "rehype-sanitize";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import { createYuxiAdapter } from "../runtime/yuxi-adapter";
import type { ChatCompletion, LocalMessage, PendingChatAttachment } from "../types";
import { sanitizeVisibleModelText } from "../utils/reasoning-visibility";
import { normalizeRichAnswer } from "../utils/rich-answer";
import { CodeHeader, HtmlFencedCodeBlock, HtmlPreviewCodeBlock, MermaidDiagram, PrismCodeBlock } from "./RichCodeBlocks";
import { YuxiAttachmentAdapter } from "../runtime/yuxi-attachment-adapter";

type Props = {
  threadId: string;
  messages: LocalMessage[];
  onRunState: (state: { runId?: string; status: string; message?: string }, threadId: string) => void;
  onCompleted: (completion: ChatCompletion, threadId: string) => void;
  bridgeAttachment?: PendingChatAttachment;
  onBridgeConsumed?: () => void;
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
    attachments: message.role === "user"
      ? (message.attachments ?? []).map((attachment) => ({
          id: attachment.id,
          type: attachment.contentType?.startsWith("image/") ? "image" : "document",
          name: attachment.name,
          contentType: attachment.contentType,
          status: { type: "complete" as const },
          content: [],
        }))
      : undefined,
  }));
}

function UserMessage() {
  return (
    <MessagePrimitive.Root className="message-row user-message-row">
      <div className="message user-message">
        <MessagePrimitive.Attachments components={{ Attachment: SentAttachment }} />
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

// 与 Web 端 DOMPurify 策略对齐：允许展示型 HTML 与行内样式，剥离脚本与事件属性。
const markdownSanitizeSchema = {
  ...defaultSchema,
  attributes: {
    ...defaultSchema.attributes,
    "*": [...(defaultSchema.attributes?.["*"] ?? [])],
    img: [...(defaultSchema.attributes?.img ?? []), "loading", "width", "height"],
  },
};

function MarkdownText() {
  return (
    <MarkdownTextPrimitive
      remarkPlugins={[remarkGfm, remarkMath]}
      rehypePlugins={[rehypeRaw, [rehypeSanitize, markdownSanitizeSchema], rehypeKatex]}
      preprocess={normalizeRichAnswer}
      components={{
        SyntaxHighlighter: PrismCodeBlock,
        CodeHeader,
        a: ({ children, ...props }) => <a {...props} target="_blank" rel="noreferrer noopener">{children}</a>,
        img: ({ alt, ...props }) => <img {...props} alt={alt || "回答中的图片"} loading="lazy" />,
      }}
      componentsByLanguage={{
        htmlpreview: { SyntaxHighlighter: HtmlPreviewCodeBlock, CodeHeader: () => null },
        // 模型输出的 ```html 围栏（常为内联样式卡片）默认渲染预览，可切换源码
        html: { SyntaxHighlighter: HtmlFencedCodeBlock, CodeHeader: () => null },
        mermaid: { SyntaxHighlighter: MermaidDiagram, CodeHeader: () => null },
      }}
      defer
    />
  );
}

function SentAttachment() {
  return (
    <AttachmentPrimitive.Root className="message-attachment">
      <Paperclip size={14} />
      <AttachmentPrimitive.Name />
    </AttachmentPrimitive.Root>
  );
}

function ComposerAttachment() {
  return (
    <AttachmentPrimitive.Root className="composer-attachment">
      <Paperclip size={14} />
      <AttachmentPrimitive.Name />
      <AttachmentPrimitive.Remove className="attachment-remove" aria-label="移除附件"><X size={14} /></AttachmentPrimitive.Remove>
    </AttachmentPrimitive.Root>
  );
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

function Composer({ bridgeAttachment, onBridgeConsumed }: Pick<Props, "bridgeAttachment" | "onBridgeConsumed">) {
  return (
    <div className="composer-shell">
      {bridgeAttachment && (
        <div className="artifact-bridge-pending" role="status">
          <Paperclip size={15} />
          <span><strong>来自科研工作流</strong><small>{bridgeAttachment.fileName} · 将随下一条问题发送</small></span>
          <button onClick={onBridgeConsumed} aria-label="移除工作流产物"><X size={14} /></button>
        </div>
      )}
      <ComposerPrimitive.Root className="composer">
        <ComposerPrimitive.Attachments components={{ Attachment: ComposerAttachment }} />
        <ComposerPrimitive.AddAttachment className="attachment-button" aria-label="添加图片或附件" multiple>
          <Paperclip size={19} />
        </ComposerPrimitive.AddAttachment>
        <ComposerPrimitive.Input
          className="composer-input"
          placeholder="询问问题，或添加图片、PDF、表格与科研附件……"
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

function RuntimeThread({ threadId, messages, onRunState, onCompleted, bridgeAttachment, onBridgeConsumed }: Props) {
  const attachmentAdapter = useMemo(() => new YuxiAttachmentAdapter(), []);
  const adapter = useMemo(
    () =>
      createYuxiAdapter(threadId, {
        // 附带本组件的 threadId，让上层能区分完成事件来自哪个会话
        //（切换会话后旧 run 的迟到回调不应污染新会话状态）。
        onRunState: (state) => onRunState(state, threadId),
        onCompleted: (completion) => onCompleted(completion, threadId),
        bridgeAttachment,
        onBridgeConsumed,
      }),
    [threadId, onRunState, onCompleted, bridgeAttachment, onBridgeConsumed],
  );
  const runtime = useLocalRuntime(adapter, {
    initialMessages: toInitialMessages(messages),
    adapters: { attachments: attachmentAdapter },
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
            <Composer bridgeAttachment={bridgeAttachment} onBridgeConsumed={onBridgeConsumed} />
          </ThreadPrimitive.ViewportFooter>
        </ThreadPrimitive.Viewport>
      </ThreadPrimitive.Root>
    </AssistantRuntimeProvider>
  );
}

export function ChatWorkspace(props: Props) {
  return <RuntimeThread key={props.threadId} {...props} />;
}
