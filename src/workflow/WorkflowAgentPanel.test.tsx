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
    const composer = await screen.findByPlaceholderText("描述要在当前项目中完成的新科研任务…");
    fireEvent.change(composer, { target: { value: "读取 input/counts.csv 并汇总关键基因" } });
    fireEvent.click(screen.getByRole("button", { name: "开始任务" }));

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
    expect(screen.getAllByText("正在等待 MiniMax-M3 返回首个结果…").length).toBeGreaterThanOrEqual(2);

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
    expect(screen.getAllByText("工具执行完成，正在核验结果…").length).toBeGreaterThanOrEqual(2);

    act(() => {
      channel?.onmessage({ type: "text_delta", delta: "| 基因 | 证据数 |\n|---|---:|\n| Wx | 2 |" });
    });

    expect(screen.getByRole("table")).toHaveTextContent("Wx");
    expect(screen.getAllByText("模型正在输出结果…").length).toBeGreaterThanOrEqual(2);

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
    expect(await screen.findByText("汇总关键基因")).toBeInTheDocument();
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
    const composer = await screen.findByPlaceholderText("描述要在当前项目中完成的新科研任务…");
    fireEvent.change(composer, { target: { value: "分析本地 CSV" } });
    fireEvent.click(screen.getByRole("button", { name: "开始任务" }));

    expect(await screen.findByText("MiniMax-M3 鉴权失败（401）：API key is invalid")).toBeInTheDocument();
    expect(screen.queryByText(/正在启动并校验本地科研引擎/)).not.toBeInTheDocument();
    const startButton = screen.getByRole("button", { name: "开始任务" });
    expect(startButton).toBeDisabled();
    expect(composer).toBeEnabled();
    fireEvent.change(composer, { target: { value: "使用正确凭据重新分析" } });
    expect(startButton).toBeEnabled();
  });

  it("一个任务运行时仍可提交第二个独立任务，并用不同 run_id 路由事件", async () => {
    const first = deferred<WorkflowAgentCompletion>();
    const second = deferred<WorkflowAgentCompletion>();
    mocks.getWorkflowModelSettings.mockResolvedValue({
      provider: "openai",
      baseUrl: "https://api.deepseek.com",
      model: "deepseek-chat",
      hasApiKey: true,
    });
    mocks.listWorkflowAgentTurns.mockResolvedValue([]);
    mocks.runWorkflowAgent
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);

    render(<WorkflowAgentPanel project={PROJECT} engine={ENGINE} onFilesChanged={vi.fn()} />);
    const composer = await screen.findByPlaceholderText("描述要在当前项目中完成的新科研任务…");
    fireEvent.change(composer, { target: { value: "任务一：读取 CSV" } });
    fireEvent.click(screen.getByRole("button", { name: "开始任务" }));
    await waitFor(() => expect(mocks.runWorkflowAgent).toHaveBeenCalledTimes(1));

    fireEvent.change(composer, { target: { value: "任务二：检查报告" } });
    expect(screen.getByText("当前任务会继续在后台运行")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "开始任务" }));
    await waitFor(() => expect(mocks.runWorkflowAgent).toHaveBeenCalledTimes(2));

    const firstRunId = mocks.runWorkflowAgent.mock.calls[0][3] as string;
    const secondRunId = mocks.runWorkflowAgent.mock.calls[1][3] as string;
    expect(firstRunId).toMatch(/^wfr_[0-9a-f]{32}$/);
    expect(secondRunId).toMatch(/^wfr_[0-9a-f]{32}$/);
    expect(firstRunId).not.toBe(secondRunId);
    expect(screen.getByText("2/3 运行中")).toBeInTheDocument();
    expect(screen.getByText("任务一：读取 CSV")).toBeInTheDocument();
    expect(screen.getByText("任务二：检查报告")).toBeInTheDocument();

    act(() => {
      first.resolve({ turnId: "e1", text: "任务一完成", inputTokens: 1, outputTokens: 1, reasoningTokens: 0, changedPaths: [] });
      second.resolve({ turnId: "e2", text: "任务二完成", inputTokens: 1, outputTokens: 1, reasoningTokens: 0, changedPaths: [] });
    });
    await waitFor(() => expect(screen.getByText("0/3 运行中")).toBeInTheDocument());
  });

  it("外部运行记录聚焦请求会打开对应的运行中任务", async () => {
    const running = persistedTurn({
      id: "turn-running",
      runId: "wfr_11111111111111111111111111111111",
      prompt: "仍在运行的科研任务",
      response: "",
      status: "running",
      finishedAt: undefined,
    });
    const completed = persistedTurn({
      id: "turn-completed",
      runId: "wfr_22222222222222222222222222222222",
      prompt: "最近完成任务",
    });
    mocks.getWorkflowModelSettings.mockResolvedValue({
      provider: "openai",
      baseUrl: "https://api.deepseek.com",
      model: "deepseek-chat",
      hasApiKey: true,
    });
    mocks.listWorkflowAgentTurns.mockResolvedValue([completed, running]);

    render(
      <WorkflowAgentPanel
        project={PROJECT}
        engine={ENGINE}
        focusRunId={running.runId}
        focusRequest={1}
        onFilesChanged={vi.fn()}
      />,
    );

    expect(await screen.findByRole("button", { name: "停止此任务" })).toBeInTheDocument();
    expect(screen.getByText("后台任务正在运行，页面将自动刷新…")).toBeInTheDocument();
  });

  it("没有配置时默认展示 DeepSeek 官方推荐参数", async () => {
    mocks.getWorkflowModelSettings.mockResolvedValue(null);
    mocks.listWorkflowAgentTurns.mockResolvedValue([]);
    render(<WorkflowAgentPanel project={PROJECT} engine={ENGINE} onFilesChanged={vi.fn()} />);

    expect(await screen.findByDisplayValue("https://api.deepseek.com")).toBeInTheDocument();
    expect(screen.getByDisplayValue("deepseek-chat")).toBeInTheDocument();
    expect(screen.getByText("推荐 · 性价比高 · OpenAI 兼容协议")).toBeInTheDocument();
  });

  it("切换到 DeepSeek 端点时不复用其他供应商的旧密钥", async () => {
    mocks.getWorkflowModelSettings.mockResolvedValue({
      provider: "anthropic",
      baseUrl: "https://api.minimaxi.com/anthropic",
      model: "MiniMax-M3",
      hasApiKey: true,
      apiKeyHint: "sk-old••••",
    });
    mocks.listWorkflowAgentTurns.mockResolvedValue([]);
    render(<WorkflowAgentPanel project={PROJECT} engine={ENGINE} onFilesChanged={vi.fn()} />);

    fireEvent.click(await screen.findByRole("button", { name: "模型设置" }));
    fireEvent.click(screen.getByRole("button", { name: "应用推荐配置" }));

    const saveButton = screen.getByRole("button", { name: "保存独立配置" });
    expect(saveButton).toBeDisabled();
    fireEvent.change(screen.getByPlaceholderText("切换供应商或端点时必须输入新 Key"), {
      target: { value: "sk-deepseek-test" },
    });
    expect(saveButton).toBeEnabled();
  });
});
