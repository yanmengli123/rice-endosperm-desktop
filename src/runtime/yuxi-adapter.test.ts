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

  it("done 之后迟到的命令失败降级为 completed，不丢弃已完成回答", async () => {
    const onRunState = vi.fn();
    const onCompleted = vi.fn();
    mocks.sendMessage.mockImplementationOnce(async (_request, channel) => {
      channel.onmessage({ type: "started", runId: "run-3" });
      channel.onmessage({ type: "text", text: "水稻胚乳发育分为" });
      channel.onmessage({ type: "done", runId: "run-3", status: "completed", text: "水稻胚乳发育分为多个关键阶段。" });
      // 服务端收尾阶段抛出的异常（如清理 ContextVar 失败）污染命令返回值
      throw new Error(
        "Yuxi 返回了无法识别的数据：<Token var=<ContextVar name='yuxi_mcp_execution_context' ...>> was created in a different Context",
      );
    });
    const adapter = createYuxiAdapter("local-thread-3", { onRunState, onCompleted });
    const messages = [
      { role: "user", content: [{ type: "text", text: "hi" }] },
    ] as unknown as ThreadMessage[];
    const stream = adapter.run({
      messages,
      abortSignal: new AbortController().signal,
    } as never) as AsyncGenerator<ChatModelRunResult>;

    const rendered: string[] = [];
    await expect(
      (async () => {
        for await (const update of stream) {
          const part = update.content?.[0];
          if (part?.type === "text") rendered.push(part.text);
        }
      })(),
    ).resolves.toBeUndefined(); // 不应向调用方抛错

    expect(rendered[rendered.length - 1]).toBe("水稻胚乳发育分为多个关键阶段。");
    const finalState = onRunState.mock.calls[onRunState.mock.calls.length - 1]?.[0];
    expect(finalState.status).toBe("completed");
    expect(finalState.message).toContain("服务端附加信息");
  });

  it("没有任何内容产出时的失败仍然抛出，保持错误可见", async () => {
    mocks.sendMessage.mockImplementationOnce(async () => {
      throw new Error("Yuxi 返回了无法识别的数据：garbage-frame");
    });
    const onRunState = vi.fn();
    const adapter = createYuxiAdapter("local-thread-4", { onRunState });
    const messages = [
      { role: "user", content: [{ type: "text", text: "hi" }] },
    ] as unknown as ThreadMessage[];
    const stream = adapter.run({
      messages,
      abortSignal: new AbortController().signal,
    } as never) as AsyncGenerator<ChatModelRunResult>;

    await expect(async () => {
      for await (const _update of stream) {
        // 消费即抛
      }
    }).rejects.toThrow("无法识别的数据");
    const finalState = onRunState.mock.calls[onRunState.mock.calls.length - 1]?.[0];
    expect(finalState.status).toBe("failed");
  });

  it("failed 终态带已落库回答时保留内容并如实显示失败", async () => {
    const onRunState = vi.fn();
    mocks.sendMessage.mockImplementationOnce(async (_request, channel) => {
      channel.onmessage({ type: "started", runId: "run-5" });
      channel.onmessage({ type: "text", text: "已生成并保存的回答" });
      channel.onmessage({
        type: "done",
        runId: "run-5",
        status: "failed",
        text: "已生成并保存的回答",
        context: {},
      });
      throw new Error("服务端收尾失败");
    });
    const adapter = createYuxiAdapter("local-thread-5", { onRunState });
    const stream = adapter.run({
      messages: [{ role: "user", content: [{ type: "text", text: "hi" }] }],
      abortSignal: new AbortController().signal,
    } as never) as AsyncGenerator<ChatModelRunResult>;

    const rendered: string[] = [];
    for await (const update of stream) {
      const part = update.content?.[0];
      if (part?.type === "text") rendered.push(part.text);
    }

    expect(rendered[rendered.length - 1]).toBe("已生成并保存的回答");
    const finalState = onRunState.mock.calls[onRunState.mock.calls.length - 1]?.[0];
    expect(finalState.status).toBe("failed");
    expect(finalState.message).toContain("回答内容已保存");
  });
});
