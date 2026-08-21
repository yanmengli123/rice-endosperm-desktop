import { Channel } from "@tauri-apps/api/core";
import type { ChatModelAdapter, ThreadMessage } from "@assistant-ui/react";
import { cancelRun, normalizeCommandError, sendMessage } from "../services/tauri-client";
import type { RunEvent } from "../types";

type AdapterCallbacks = {
  onRunState?: (state: { runId?: string; status: string; message?: string }) => void;
  onCompleted?: () => void;
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
      if (!question) throw new Error("请输入问题后再发送");

      const request = {
        threadId: localThreadId,
        question,
        requestId: requestId(),
      };
      const queue = new AsyncQueue<RunEvent>();
      const channel = new Channel<RunEvent>();
      let activeRunId: string | undefined;
      let accumulatedText = "";

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
            accumulatedText = event.text;
            yield { content: [{ type: "text", text: accumulatedText }] };
          } else if (event.type === "done") {
            accumulatedText = event.text;
            yield { content: [{ type: "text", text: accumulatedText }] };
          }
        }
        const completion = await invocation;
        if (completion.text !== accumulatedText) {
          accumulatedText = completion.text;
          yield { content: [{ type: "text", text: accumulatedText }] };
        }
        callbacks.onRunState?.({ runId: activeRunId, status: "completed" });
        callbacks.onCompleted?.();
      } catch (error) {
        if (abortSignal.aborted || (error instanceof DOMException && error.name === "AbortError")) {
          callbacks.onRunState?.({ runId: activeRunId, status: "cancelled" });
          return;
        }
        callbacks.onRunState?.({
          runId: activeRunId,
          status: "failed",
          message: normalizeCommandError(error).message,
        });
        throw error;
      } finally {
        abortSignal.removeEventListener("abort", abort);
      }
    },
  };
}
