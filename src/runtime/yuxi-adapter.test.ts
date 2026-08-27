// @vitest-environment jsdom

import type { ChatModelRunResult, ThreadMessage } from "@assistant-ui/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createYuxiAdapter } from "./yuxi-adapter";

const mocks = vi.hoisted(() => ({
  cancelRun: vi.fn(),
  sendMessage: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class<T> {
    onmessage: (event: T) => void = () => undefined;
  },
}));

vi.mock("../services/tauri-client", () => ({
  cancelRun: mocks.cancelRun,
  sendMessage: mocks.sendMessage,
  normalizeCommandError: (error: unknown) =>
    error instanceof Error ? error : new Error("命令失败"),
}));

describe("createYuxiAdapter", () => {
  afterEach(() => {
    vi.clearAllMocks();
  });

  it("始终用命令返回的服务端权威结果覆盖中间流", async () => {
    mocks.sendMessage.mockImplementationOnce(async (_request, channel) => {
      channel.onmessage({ type: "text", text: "检索中的模型过程" });
      return {
        runId: "run-1",
        threadId: "yuxi-thread-1",
        requestId: "desktop-request-1",
        status: "completed",
        text: "Yuxi 服务端最终回答",
        context: {
          protocolVersion: "1.1",
          modelSpec: "minimax-cn:MiniMax-M3",
          knowledgeScope: { allowWeb: false, kbCount: 3, members: [] },
          knowledgeRetrievals: [],
        },
      };
    });
    const onCompleted = vi.fn();
    const adapter = createYuxiAdapter("local-thread-1", { onCompleted });
    const messages = [
      {
        role: "user",
        content: [{ type: "text", text: "水稻胚乳发育的关键调控基因有哪些？" }],
      },
    ] as unknown as ThreadMessage[];

    const stream = adapter.run({
      messages,
      abortSignal: new AbortController().signal,
    } as never) as AsyncGenerator<ChatModelRunResult>;
    const rendered: string[] = [];
    for await (const update of stream) {
      const part = update.content?.[0];
      if (part?.type === "text") rendered.push(part.text);
    }

    expect(rendered).toEqual(["检索中的模型过程", "Yuxi 服务端最终回答"]);
    expect(onCompleted).toHaveBeenCalledWith(
      expect.objectContaining({
        text: "Yuxi 服务端最终回答",
        context: expect.objectContaining({ modelSpec: "minimax-cn:MiniMax-M3" }),
      }),
    );
  });

  it("从流式预览、最终结果和完成回调中移除 think 内容", async () => {
    mocks.sendMessage.mockImplementationOnce(async (_request, channel) => {
      channel.onmessage({ type: "text", text: "<think>private chain" });
      channel.onmessage({ type: "text", text: "<think>private chain</think>你好！" });
      return {
        runId: "run-2",
        threadId: "yuxi-thread-2",
        requestId: "desktop-request-2",
        status: "completed",
        text: "\\<think>private final chain</think>你好！我是稻芯智析。",
        context: {
          protocolVersion: "1.1",
          knowledgeScope: { allowWeb: false, kbCount: 0, members: [] },
          knowledgeRetrievals: [],
        },
      };
    });
    const onCompleted = vi.fn();
    const adapter = createYuxiAdapter("local-thread-2", { onCompleted });
    const messages = [
      { role: "user", content: [{ type: "text", text: "hi" }] },
    ] as unknown as ThreadMessage[];
    const stream = adapter.run({
      messages,
      abortSignal: new AbortController().signal,
    } as never) as AsyncGenerator<ChatModelRunResult>;

    const rendered: string[] = [];
    for await (const update of stream) {
      const part = update.content?.[0];
      if (part?.type === "text") rendered.push(part.text);
    }

    expect(rendered).not.toContain(expect.stringContaining("private"));
    expect(rendered[rendered.length - 1]).toBe("你好！我是稻芯智析。");
    expect(onCompleted).toHaveBeenCalledWith(
      expect.objectContaining({ text: "你好！我是稻芯智析。" }),
    );
  });
});
