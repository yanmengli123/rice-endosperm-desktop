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
  // 忠实镜像真实 normalizeCommandError：保留结构化错误的 message/code/action，
  // 非对象输入回退默认文案。
  normalizeCommandError: (error: unknown) => {
    const normalized = new Error(
      typeof error === "object" && error !== null && "message" in error
        ? String((error as { message: unknown }).message)
        : "命令失败",
    ) as Error & { code?: string; action?: string };
    if (typeof error === "object" && error !== null) {
      const source = error as { code?: unknown; action?: unknown };
      if (typeof source.code === "string") normalized.code = source.code;
      if (typeof source.action === "string") normalized.action = source.action;
    }
    return normalized;
  },
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

  it("失败态把服务端引导动作（action）贯通给 UI 渲染可执行入口", async () => {
    mocks.sendMessage.mockImplementationOnce(async () => {
      throw {
        message: "当前策略要求使用自有模型凭据，请先在设置中配置后再发起对话",
        code: "byok_required",
        action: "configure_byok",
        retryable: false,
      } as unknown as Error;
    });
    const onRunState = vi.fn();
    const adapter = createYuxiAdapter("local-thread-action", { onRunState });
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
    }).rejects.toThrow("自有模型凭据");
    const finalState = onRunState.mock.calls[onRunState.mock.calls.length - 1]?.[0];
    expect(finalState.status).toBe("failed");
    expect(finalState.action).toBe("configure_byok");
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

  it("把当前用户消息的 Yuxi 附件元数据传入原生 AgentRun 请求", async () => {
    mocks.sendMessage.mockResolvedValueOnce({
      runId: "run-attachment",
      threadId: "server-thread",
      requestId: "desktop-attachment",
      status: "completed",
      text: "附件分析完成",
      context: { knowledgeScope: { allowWeb: false, kbCount: 0, members: [] }, knowledgeRetrievals: [] },
    });
    const attachment = {
      tmpFileId: "tmp-1",
      fileName: "endosperm.pdf",
      fileType: "application/pdf",
      fileSize: 1024,
      bucketName: "knowledgebases",
      objectName: "tmp/chat_attachments/u1/tmp-1/original/endosperm.pdf",
      parseSupported: true,
      parseMethods: ["disable"],
      parsedObjectName: "tmp/chat_attachments/u1/tmp-1/parsed/endosperm.md",
      parseMethod: "disable",
      truncated: false,
    };
    const messages = [{
      role: "user",
      content: [{ type: "text", text: "总结这篇文献" }],
      attachments: [{
        id: "attachment-1",
        type: "document",
        name: "endosperm.pdf",
        status: { type: "complete" },
        content: [{ type: "data", name: "yuxi-chat-attachment", data: attachment }],
      }],
    }] as unknown as ThreadMessage[];
    const stream = createYuxiAdapter("local-thread").run({
      messages,
      abortSignal: new AbortController().signal,
    } as never) as AsyncGenerator<ChatModelRunResult>;
    for await (const _update of stream) {
      // consume
    }
    expect(mocks.sendMessage).toHaveBeenCalledWith(
      expect.objectContaining({ attachments: [attachment], question: "总结这篇文献" }),
      expect.anything(),
    );
  });

  it("仅在问答成功后消费用户显式桥接的工作流产物", async () => {
    const bridgeAttachment = {
      tmpFileId: "tmp-workflow-1",
      fileName: "PCA.csv",
      fileType: "text/csv",
      fileSize: 2048,
      bucketName: "knowledgebases",
      objectName: "tmp/chat_attachments/u1/tmp-workflow-1/original/PCA.csv",
      parseSupported: false,
      parseMethods: [],
      truncated: false,
    };
    mocks.sendMessage.mockResolvedValueOnce({
      runId: "run-workflow-bridge",
      threadId: "server-thread",
      requestId: "desktop-workflow-bridge",
      status: "completed",
      text: "PCA 结果分析完成",
      context: { knowledgeScope: { allowWeb: false, kbCount: 0, members: [] }, knowledgeRetrievals: [] },
    });
    const onBridgeConsumed = vi.fn();
    const stream = createYuxiAdapter("local-thread", {
      bridgeAttachment,
      onBridgeConsumed,
    }).run({
      messages: [{ role: "user", content: [{ type: "text", text: "解释这个 PCA 结果" }] }],
      abortSignal: new AbortController().signal,
    } as never) as AsyncGenerator<ChatModelRunResult>;
    for await (const _update of stream) {
      // consume
    }
    expect(mocks.sendMessage).toHaveBeenCalledWith(
      expect.objectContaining({ attachments: [bridgeAttachment] }),
      expect.anything(),
    );
    expect(onBridgeConsumed).toHaveBeenCalledOnce();
  });

  it("把 ask_user_question 事件透传给 UI 渲染澄清卡片", async () => {
    const questions = [{
      questionId: "q-stage",
      question: "重点分析哪个发育阶段？",
      options: [{ label: "灌浆期", value: "灌浆期" }, { label: "成熟期", value: "成熟期" }],
      multiSelect: false,
      allowOther: true,
    }];
    mocks.sendMessage.mockImplementationOnce(async (_request, channel) => {
      channel.onmessage({ type: "status", status: "running", message: "分析中" });
      channel.onmessage({ type: "ask_user_question", runId: "run-q", questions });
      return {
        runId: "run-q",
        threadId: "server-thread",
        requestId: "desktop-request-q",
        status: "interrupted",
        text: "",
        context: { knowledgeScope: { allowWeb: false, kbCount: 0, members: [] }, knowledgeRetrievals: [] },
      };
    });
    const onAskUserQuestion = vi.fn();
    const stream = createYuxiAdapter("local-thread", { onAskUserQuestion }).run({
      messages: [{ role: "user", content: [{ type: "text", text: "分析胚乳发育" }] }],
      abortSignal: new AbortController().signal,
    } as never) as AsyncGenerator<ChatModelRunResult>;
    for await (const _update of stream) {
      // consume
    }
    expect(onAskUserQuestion).toHaveBeenCalledWith("run-q", questions);
  });

  it("澄清问题续跑把 resume 三元组（runId/questions/answers）传入请求", async () => {
    mocks.sendMessage.mockResolvedValueOnce({
      runId: "run-resume-q",
      threadId: "server-thread",
      requestId: "desktop-resume-q",
      status: "completed",
      text: "续跑完成",
      context: { knowledgeScope: { allowWeb: false, kbCount: 0, members: [] }, knowledgeRetrievals: [] },
    });
    const questions = [{
      questionId: "q-stage",
      question: "重点分析哪个发育阶段？",
      options: [{ label: "灌浆期", value: "灌浆期" }],
      multiSelect: false,
      allowOther: true,
    }];
    const answers = [{ questionId: "q-stage", selected: ["__other__"], otherText: "授粉后 15 天" }];
    let resumeRunId = "run-q-parent";
    let resumeQuestions = questions;
    let resumeAnswers = answers;
    const stream = createYuxiAdapter("local-thread", {
      consumeResumeRunId: () => {
        const pending = resumeRunId;
        resumeRunId = "";
        return pending || undefined;
      },
      consumeResumeQuestions: () => {
        const pending = resumeQuestions;
        resumeQuestions = [];
        return pending.length > 0 ? pending : undefined;
      },
      consumeResumeAnswers: () => {
        const pending = resumeAnswers;
        resumeAnswers = [];
        return pending.length > 0 ? pending : undefined;
      },
    }).run({
      messages: [{ role: "user", content: [{ type: "text", text: "灌浆期" }] }],
      abortSignal: new AbortController().signal,
    } as never) as AsyncGenerator<ChatModelRunResult>;
    for await (const _update of stream) {
      // consume
    }
    expect(mocks.sendMessage).toHaveBeenCalledWith(
      expect.objectContaining({
        resumeRunId: "run-q-parent",
        resumeQuestions: questions,
        resumeAnswers: answers,
        attachments: [],
      }),
      expect.anything(),
    );
  });

  it("把 citations_ready 事件的引用与图卡透传给 UI", async () => {
    const citation = {
      status: "VERIFIED",
      evidenceId: "ev-1",
      fileId: "file-0001",
      filename: "rice.pdf",
      zone: "figure",
      page: 3,
      kbId: "kb-0001",
      revisionId: "rev-0012",
    };
    const figures = [{
      bindingId: "bind-1",
      kbId: "kb-0001",
      fileId: "file-0001",
      revisionId: "rev-0012",
      assetName: "fig-3.png",
      mediaType: "image/png",
      page: 3,
      role: "primary",
      figureLabel: "Figure 3",
    }];
    mocks.sendMessage.mockImplementationOnce(async (_request, channel) => {
      channel.onmessage({ type: "citations_ready", runId: "run-ev", citation, figures });
      return {
        runId: "run-ev",
        threadId: "server-thread",
        requestId: "desktop-request-ev",
        status: "completed",
        text: "回答",
        context: { knowledgeScope: { allowWeb: false, kbCount: 0, members: [] }, knowledgeRetrievals: [] },
      };
    });
    const onCitationsReady = vi.fn();
    const stream = createYuxiAdapter("local-thread", { onCitationsReady }).run({
      messages: [{ role: "user", content: [{ type: "text", text: "Figure 3 展示了什么？" }] }],
      abortSignal: new AbortController().signal,
    } as never) as AsyncGenerator<ChatModelRunResult>;
    for await (const _update of stream) {
      // consume
    }
    expect(onCitationsReady).toHaveBeenCalledWith("run-ev", citation, figures, [], []);
  });

  it("把 locator_candidates 候选清单透传给 UI", async () => {
    const candidates = [
      { fileId: "file-a", kbId: "kb-1", filename: "paper-a.pdf" },
      { fileId: "file-b", kbId: "kb-1", filename: "paper-b.pdf" },
    ];
    mocks.sendMessage.mockImplementationOnce(async (_request, channel) => {
      channel.onmessage({ type: "locator_candidates", runId: "run-loc", candidates });
      return {
        runId: "run-loc",
        threadId: "server-thread",
        requestId: "desktop-request-loc",
        status: "completed",
        text: "回答",
        context: { knowledgeScope: { allowWeb: false, kbCount: 0, members: [] }, knowledgeRetrievals: [] },
      };
    });
    const onLocatorCandidates = vi.fn();
    const stream = createYuxiAdapter("local-thread", { onLocatorCandidates }).run({
      messages: [{ role: "user", content: [{ type: "text", text: "那张图说明了什么？" }] }],
      abortSignal: new AbortController().signal,
    } as never) as AsyncGenerator<ChatModelRunResult>;
    for await (const _update of stream) {
      // consume
    }
    expect(onLocatorCandidates).toHaveBeenCalledWith("run-loc", candidates);
  });

  it("把 warning 事件作为非阻塞提示透传给 UI", async () => {
    mocks.sendMessage.mockImplementationOnce(async (_request, channel) => {
      channel.onmessage({ type: "warning", message: "部分检索来源超时" });
      return {
        runId: "run-w",
        threadId: "server-thread",
        requestId: "desktop-request-w",
        status: "completed",
        text: "回答",
        context: { knowledgeScope: { allowWeb: false, kbCount: 0, members: [] }, knowledgeRetrievals: [] },
      };
    });
    const onWarning = vi.fn();
    const stream = createYuxiAdapter("local-thread", { onWarning }).run({
      messages: [{ role: "user", content: [{ type: "text", text: "分析胚乳发育" }] }],
      abortSignal: new AbortController().signal,
    } as never) as AsyncGenerator<ChatModelRunResult>;
    for await (const _update of stream) {
      // consume
    }
    expect(onWarning).toHaveBeenCalledWith("部分检索来源超时");
  });
});
