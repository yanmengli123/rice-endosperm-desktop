// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type {
  WorkflowAgentCompletion,
  WorkflowAgentEvent,
  WorkflowAgentTurn,
  WorkflowProject,
} from "../types";
import { WorkflowAgentPanel } from "./WorkflowAgentPanel";

const mocks = vi.hoisted(() => ({
  channels: [] as Array<{ onmessage: (event: WorkflowAgentEvent) => void }>,
  getWorkflowModelSettings: vi.fn(),
  listWorkflowAgentTurns: vi.fn(),
  runWorkflowAgent: vi.fn(),
  cancelWorkflowAgent: vi.fn(),
  deleteWorkflowModelSettings: vi.fn(),
  respondWorkflowApproval: vi.fn(),
  saveWorkflowModelSettings: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class<T> {
    onmessage: (event: T) => void = () => undefined;

    constructor() {
      mocks.channels.push(this as unknown as { onmessage: (event: WorkflowAgentEvent) => void });
    }
  },
}));

vi.mock("../services/tauri-client", () => ({
  cancelWorkflowAgent: mocks.cancelWorkflowAgent,
  deleteWorkflowModelSettings: mocks.deleteWorkflowModelSettings,
  getWorkflowModelSettings: mocks.getWorkflowModelSettings,
  listWorkflowAgentTurns: mocks.listWorkflowAgentTurns,
  normalizeCommandError: (error: unknown) =>
    error instanceof Error ? error : new Error("工作流命令失败"),
  respondWorkflowApproval: mocks.respondWorkflowApproval,
  runWorkflowAgent: mocks.runWorkflowAgent,
  saveWorkflowModelSettings: mocks.saveWorkflowModelSettings,
}));

const PROJECT: WorkflowProject = {
  id: "project-1",
  name: "水稻胚乳表达矩阵",
  root: "D:\\research\\rice",
  createdAt: "2026-09-02T00:00:00Z",
  updatedAt: "2026-09-02T00:00:00Z",
};

const ENGINE = {
  protocol: "rice.workflow.v1",
  available: true,
  runningProjects: 0,
  workerVersion: "wisp-test",
  message: "ready",
};

function persistedTurn(overrides: Partial<WorkflowAgentTurn> = {}): WorkflowAgentTurn {
  return {
    id: "turn-local-1",
    runId: "run-1",
    projectId: PROJECT.id,
    provider: "anthropic",
    model: "MiniMax-M3",
    prompt: "汇总关键基因",
    response: "## 已持久化结果\n\n| 基因 | 证据数 |\n|---|---:|\n| Wx | 3 |",
    status: "completed",
    inputTokens: 120,
    outputTokens: 40,
    reasoningTokens: 0,
    createdAt: "2026-09-02T00:01:00Z",
    finishedAt: "2026-09-02T00:02:00Z",
    ...overrides,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

describe("WorkflowAgentPanel", () => {
  afterEach(() => {
    cleanup();
    mocks.channels.length = 0;
    vi.clearAllMocks();
  });

  it("把模型流式结果显示在页面，并用命令返回的权威结果完成 Markdown 表格渲染", async () => {
    const completion = deferred<WorkflowAgentCompletion>();
    const saved = persistedTurn();
    mocks.getWorkflowModelSettings.mockResolvedValue({
      provider: "anthropic",
      baseUrl: "https://api.minimaxi.com/anthropic",
      model: "MiniMax-M3",
      hasApiKey: true,
      apiKeyHint: "sk-••••••••",
    });
    mocks.listWorkflowAgentTurns
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([saved]);
    mocks.runWorkflowAgent.mockReturnValue(completion.promise);
    const onFilesChanged = vi.fn();

    render(<WorkflowAgentPanel project={PROJECT} engine={ENGINE} onFilesChanged={onFilesChanged} />);
    const composer = await screen.findByPlaceholderText("描述要在当前项目中完成的科研计算任务…");
    fireEvent.change(composer, { target: { value: "读取 input/counts.csv 并汇总关键基因" } });
    fireEvent.click(screen.getByRole("button", { name: "执行" }));

    await waitFor(() => expect(mocks.runWorkflowAgent).toHaveBeenCalledOnce());
    const channel = mocks.channels[mocks.channels.length - 1];
    expect(channel).toBeDefined();
    act(() => {
      channel?.onmessage({
        type: "progress",
        phase: "waiting_model",
        message: "正在等待 MiniMax-M3 返回首个结果…",
        elapsed_ms: 5000,
      });
    });
    expect(screen.getByText("正在等待 MiniMax-M3 返回首个结果…")).toBeInTheDocument();

    act(() => {
      channel?.onmessage({
        type: "tool_started",
        call_id: "read-1",
        name: "read",
        preview: "input/counts.csv",
      });
      channel?.onmessage({
        type: "tool_finished",
        call_id: "read-1",
        name: "read",
        ok: true,
        content: "3 rows",
        duration_ms: 12,
      });
    });
    expect(screen.getByText("工具执行完成，正在核验结果…")).toBeInTheDocument();

    act(() => {
      channel?.onmessage({ type: "text_delta", delta: "| 基因 | 证据数 |\n|---|---:|\n| Wx | 2 |" });
    });

    expect(screen.getByRole("table")).toHaveTextContent("Wx");
    expect(screen.getByText("模型正在输出结果…")).toBeInTheDocument();

    act(() => {
      completion.resolve({
        turnId: "turn-engine-1",
        text: saved.response,
        sessionId: "session-1",
        inputTokens: 120,
        outputTokens: 40,
        reasoningTokens: 0,
        changedPaths: ["results/summary.md"],
      });
    });

    expect(await screen.findByRole("heading", { name: "已持久化结果" })).toBeInTheDocument();
    expect(screen.getByRole("table")).toHaveTextContent("Wx3");
    expect(await screen.findByText("已持久化回合")).toBeInTheDocument();
    expect(onFilesChanged).toHaveBeenCalled();
  });

  it("重新进入或切换项目时自动恢复最近一个非空持久化答案", async () => {
    const firstTurn = persistedTurn();
    const secondProject = { ...PROJECT, id: "project-2", name: "第二个项目" };
    const secondTurn = persistedTurn({
      id: "turn-local-2",
      projectId: secondProject.id,
      prompt: "恢复第二个项目",
      response: "## 第二个项目结果\n\n本地文件分析已完成。",
    });
    mocks.getWorkflowModelSettings.mockResolvedValue({
      provider: "anthropic",
      baseUrl: "https://api.minimaxi.com/anthropic",
      model: "MiniMax-M3",
      hasApiKey: true,
    });
    mocks.listWorkflowAgentTurns.mockImplementation(async (projectId: string) =>
      projectId === PROJECT.id ? [firstTurn] : [secondTurn],
    );

    const { rerender } = render(
      <WorkflowAgentPanel project={PROJECT} engine={ENGINE} onFilesChanged={vi.fn()} />,
    );
    expect(await screen.findByRole("heading", { name: "已持久化结果" })).toBeInTheDocument();
    expect(screen.getByRole("table")).toHaveTextContent("Wx3");

    rerender(<WorkflowAgentPanel project={secondProject} engine={ENGINE} onFilesChanged={vi.fn()} />);
    expect(await screen.findByRole("heading", { name: "第二个项目结果" })).toBeInTheDocument();
    expect(screen.getByText("本地文件分析已完成。")).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "已持久化结果" })).not.toBeInTheDocument();
  });

  it("模型鉴权失败时结束思考状态并在页面明确显示错误", async () => {
    mocks.getWorkflowModelSettings.mockResolvedValue({
      provider: "anthropic",
      baseUrl: "https://api.minimaxi.com/anthropic",
      model: "MiniMax-M3",
      hasApiKey: true,
    });
    mocks.listWorkflowAgentTurns.mockResolvedValue([]);
    mocks.runWorkflowAgent.mockRejectedValue(new Error("MiniMax-M3 鉴权失败（401）：API key is invalid"));

    render(<WorkflowAgentPanel project={PROJECT} engine={ENGINE} onFilesChanged={vi.fn()} />);
    const composer = await screen.findByPlaceholderText("描述要在当前项目中完成的科研计算任务…");
    fireEvent.change(composer, { target: { value: "分析本地 CSV" } });
    fireEvent.click(screen.getByRole("button", { name: "执行" }));

    expect(await screen.findByText("MiniMax-M3 鉴权失败（401）：API key is invalid")).toBeInTheDocument();
    expect(screen.queryByText(/正在启动并校验本地科研引擎/)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "执行" })).toBeEnabled();
  });
});
