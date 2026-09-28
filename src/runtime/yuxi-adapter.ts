import { Channel } from "@tauri-apps/api/core";
import type { ChatModelAdapter, ThreadMessage } from "@assistant-ui/react";
import { cancelRun, normalizeCommandError, sendMessage } from "../services/tauri-client";
import type {
  ChatCompletion,
  CitationRef,
  ClarificationQuestion,
  FigureCardAsset,
  FigureRef,
  PublishableTable,
  GraphSnapshot,
  LocatorCandidate,
  QuestionAnswer,
  RunEvent,
  TraceEvent,
} from "../types";
import { sanitizeVisibleModelText } from "../utils/reasoning-visibility";
import { YUXI_ATTACHMENT_PART_NAME } from "./yuxi-attachment-adapter";
import type { PendingChatAttachment } from "../types";

type AdapterCallbacks = {
  onRunState?: (state: { runId?: string; status: string; message?: string; action?: string }) => void;
  onCompleted?: (completion: ChatCompletion) => void;
  /** 执行轨迹事件（yuxi.run-trace.v1 wire 原样透传），由 UI 层投影渲染。 */
  onTraceEvent?: (event: TraceEvent) => void;
  /** 澄清问题中断（ask_user_question_required）：UI 渲染选项卡片。 */
  onAskUserQuestion?: (runId: string, questions: ClarificationQuestion[]) => void;
  /** 证据平面：结构化引用（citation_ready v2 九键）与可选图卡投影。 */
  onCitationsReady?: (
    runId: string,
    citation: CitationRef,
    figures: FigureCardAsset[],
    figureRefs: FigureRef[],
    tables: PublishableTable[],
  ) => void;
  /** 跨文献歧义候选清单（locator_candidates）：展示用。 */
  onLocatorCandidates?: (runId: string, candidates: LocatorCandidate[]) => void;
  /** 确定性规范关系子图（graph_snapshot_v1）。 */
  onGraphSnapshotReady?: (runId: string, snapshot: GraphSnapshot) => void;
  /** 运行内非阻塞提示（yuxi.warning）。 */
  onWarning?: (message: string) => void;
  bridgeAttachment?: PendingChatAttachment;
  onBridgeConsumed?: () => void;
  /** 取走待续跑的父 run ID（人工审批/澄清问题续跑）：返回后即清除，下一次发送是普通提问。 */
  consumeResumeRunId?: () => string | undefined;
  /** 取走待续跑的澄清问题定义与用户作答（与 resumeRunId 成对消费）。 */
  consumeResumeQuestions?: () => ClarificationQuestion[] | undefined;
  consumeResumeAnswers?: () => QuestionAnswer[] | undefined;
};

class AsyncQueue<T> {
  private values: T[] = [];
  private waiters: Array<(value: IteratorResult<T>) => void> = [];
  private closed = false;
  private failure?: Error;

  push(value: T) {
    const waiter = this.waiters.shift();
    if (waiter) waiter({ value, done: false });
    else this.values.push(value);
  }

  close() {
    this.closed = true;
    for (const waiter of this.waiters.splice(0)) waiter({ value: undefined, done: true });
  }

  fail(error: Error) {
    this.failure = error;
    this.close();
  }

  async *iterate(): AsyncGenerator<T> {
    while (true) {
      if (this.values.length > 0) {
        yield this.values.shift()!;
        continue;
      }
      if (this.closed) {
        if (this.failure) throw this.failure;
        return;
      }
      const next = await new Promise<IteratorResult<T>>((resolve) => this.waiters.push(resolve));
      if (next.done) {
        if (this.failure) throw this.failure;
        return;
      }
      yield next.value;
    }
  }
}

function latestUserText(messages: readonly ThreadMessage[]): string {
  const message = [...messages].reverse().find((item) => item.role === "user");
  if (!message) return "";
  return message.content
    .filter((part) => part.type === "text")
    .map((part) => (part.type === "text" ? part.text : ""))
    .join("\n")
    .trim();
}

function latestUserAttachments(messages: readonly ThreadMessage[]): PendingChatAttachment[] {
  const message = [...messages].reverse().find((item) => item.role === "user");
  if (!message || message.role !== "user") return [];
  const attachments: PendingChatAttachment[] = [];
  for (const attachment of message.attachments ?? []) {
    for (const part of attachment.content) {
      if (part.type !== "data" || part.name !== YUXI_ATTACHMENT_PART_NAME) continue;
      const value = part.data as unknown;
      if (
        typeof value === "object"
        && value !== null
        && "tmpFileId" in value
        && "objectName" in value
        && "bucketName" in value
      ) {
        attachments.push(value as PendingChatAttachment);
      }
    }
  }
  return attachments;
}

function requestId() {
  return `desktop-${crypto.randomUUID()}`;
}

export function createYuxiAdapter(
  localThreadId: string,
  callbacks: AdapterCallbacks = {},
): ChatModelAdapter {
  return {
    async *run({ messages, abortSignal }) {
      const question = latestUserText(messages);
      const attachments = latestUserAttachments(messages);
      if (
        callbacks.bridgeAttachment
        && !attachments.some((item) => item.tmpFileId === callbacks.bridgeAttachment?.tmpFileId)
      ) {
        attachments.push(callbacks.bridgeAttachment);
      }
      // 人工审批/澄清问题续跑：本轮发送是对 interrupted run 的答复——以 resume
      // 载荷发送，不携带新附件；附件此时应留给下一轮普通提问。
      const resumeRunId = callbacks.consumeResumeRunId?.();
      const effectiveAttachments = resumeRunId ? [] : attachments;
      const resumeQuestions = resumeRunId ? callbacks.consumeResumeQuestions?.() ?? [] : [];
      const resumeAnswers = resumeRunId ? callbacks.consumeResumeAnswers?.() ?? [] : [];
      if (!question) throw new Error("请输入问题或批复后再发送");
      const resolvedQuestion = question || "请分析随附文件。";

      const request = {
        threadId: localThreadId,
        question: resolvedQuestion,
        requestId: requestId(),
        attachments: effectiveAttachments,
        resumeRunId,
        resumeQuestions,
        resumeAnswers,
      };
      const queue = new AsyncQueue<RunEvent>();
      const channel = new Channel<RunEvent>();
      let activeRunId: string | undefined;
      let accumulatedText = "";
      let sawDone = false;
      let doneStatus: string | undefined;

      channel.onmessage = (event) => queue.push(event);
      const invocation = sendMessage(request, channel);
      void invocation.then(
        () => queue.close(),
        (error) => queue.fail(normalizeCommandError(error)),
      );

      const abort = () => {
        void cancelRun(request.requestId, activeRunId)
          .catch(() => undefined)
          .finally(() => {
            queue.fail(new DOMException("请求已取消", "AbortError"));
          });
      };
      abortSignal.addEventListener("abort", abort, { once: true });

      try {
        for await (const event of queue.iterate()) {
          if (event.type === "started") {
            activeRunId = event.runId;
            callbacks.onRunState?.({ runId: event.runId, status: "running" });
          } else if (event.type === "status") {
            callbacks.onRunState?.({ runId: activeRunId, status: event.status, message: event.message });
          } else if (event.type === "text") {
            accumulatedText = sanitizeVisibleModelText(event.text);
            yield { content: [{ type: "text", text: accumulatedText }] };
          } else if (event.type === "trace") {
            callbacks.onTraceEvent?.(event.trace);
          } else if (event.type === "ask_user_question") {
            callbacks.onAskUserQuestion?.(event.runId, event.questions);
          } else if (event.type === "citations_ready") {
            callbacks.onCitationsReady?.(
              event.runId,
              event.citation,
              event.figures,
              event.figureRefs ?? [],
              event.tables ?? [],
            );
          } else if (event.type === "locator_candidates") {
            callbacks.onLocatorCandidates?.(event.runId, event.candidates);
          } else if (event.type === "graph_snapshot_ready") {
            callbacks.onGraphSnapshotReady?.(event.runId, event.snapshot);
          } else if (event.type === "warning") {
            callbacks.onWarning?.(event.message);
          } else if (event.type === "done") {
            sawDone = true;
            doneStatus = event.status;
            accumulatedText = sanitizeVisibleModelText(event.text);
            yield { content: [{ type: "text", text: accumulatedText }] };
          }
        }
        const rawCompletion = await invocation;
        const completion = {
          ...rawCompletion,
          text: sanitizeVisibleModelText(rawCompletion.text),
        };
        if (completion.text !== accumulatedText) {
          accumulatedText = completion.text;
          yield { content: [{ type: "text", text: accumulatedText }] };
        }
        // interrupted 是可续跑态（等待用户批复），不是失败：如实上报，
        // 由 ChatWorkspace 呈现续跑入口。
        callbacks.onRunState?.({
          runId: activeRunId,
          status: completion.status === "interrupted" ? "interrupted" : "completed",
        });
        callbacks.onCompleted?.(completion);
        if (callbacks.bridgeAttachment) callbacks.onBridgeConsumed?.();
      } catch (error) {
        if (abortSignal.aborted || (error instanceof DOMException && error.name === "AbortError")) {
          callbacks.onRunState?.({ runId: activeRunId, status: "cancelled" });
          return;
        }
        // Rust 层只有在非空回答已成功落库后才会发送 done。即使服务端终态为
        // failed/interrupted，也保留已经送达的内容，同时如实展示服务端终态。
        if (sawDone && accumulatedText.trim().length > 0) {
          const detail = normalizeCommandError(error).message;
          const status = doneStatus === "completed" ? "completed" : (doneStatus ?? "failed");
          callbacks.onRunState?.({
            runId: activeRunId,
            status,
            message:
              status === "completed"
                ? `回答已完成并保存；服务端附加信息：${detail}`
                : `回答内容已保存；服务端终态为 ${status}：${detail}`,
          });
          return;
        }
        // 服务端结构化错误码携带的引导动作（configure_byok/contact_admin/
        // wait_for_active_run/upgrade_client）随失败态贯通到 UI，由 App 层
        // 渲染可执行动作（如深链模型设置）。
        const commandError = normalizeCommandError(error);
        callbacks.onRunState?.({
          runId: activeRunId,
          status: "failed",
          message: commandError.message,
          action: commandError.action,
        });
        throw error;
      } finally {
        abortSignal.removeEventListener("abort", abort);
      }
    },
  };
}
