// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { WorkflowProject, WorkflowRun } from "../types";
import { WorkflowWorkspace } from "./WorkflowWorkspace";

const mocks = vi.hoisted(() => ({
  listWorkflowProjects: vi.fn(),
  listWorkflowRuns: vi.fn(),
  listWorkflowArtifacts: vi.fn(),
  getWorkflowEngineStatus: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class<T> {
    onmessage: (event: T) => void = () => undefined;
  },
}));

vi.mock("../services/tauri-client", () => ({
  cancelWorkflowRun: vi.fn(),
  createWorkflowProject: vi.fn(),
  deleteWorkflowProject: vi.fn(),
  getWorkflowEngineStatus: mocks.getWorkflowEngineStatus,
  listWorkflowArtifacts: mocks.listWorkflowArtifacts,
  listWorkflowProjects: mocks.listWorkflowProjects,
  listWorkflowRuns: mocks.listWorkflowRuns,
  normalizeCommandError: (error: unknown) => error instanceof Error ? error : new Error("工作流失败"),
  openWorkflowArtifact: vi.fn(),
  pickWorkflowDirectory: vi.fn(),
  runCountsPcaWorkflow: vi.fn(),
}));

vi.mock("./WorkflowAgentPanel", () => ({
  WorkflowAgentPanel: ({ focusRunId }: { focusRunId?: string }) => (
    <div data-testid="workflow-agent-panel" data-focused-run={focusRunId || ""} />
  ),
}));

const PROJECT: WorkflowProject = {
  id: "project-1",
  name: "水稻胚乳项目",
  root: "D:\\research\\rice",
  createdAt: "2026-09-02T00:00:00Z",
  updatedAt: "2026-09-02T00:00:00Z",
};

const RUN: WorkflowRun = {
  id: "wfr_0123456789abcdef0123456789abcdef",
  projectId: PROJECT.id,
  workflowKind: "wisp-agent",
  status: "running",
  summaryJson: "{}",
  createdAt: "2026-09-02T00:01:00Z",
  startedAt: "2026-09-02T00:01:00Z",
};

describe("WorkflowWorkspace", () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("点击运行中的 WISP 记录会切回科研助手并聚焦对应任务", async () => {
    mocks.listWorkflowProjects.mockResolvedValue([PROJECT]);
    mocks.listWorkflowRuns.mockResolvedValue([RUN]);
    mocks.listWorkflowArtifacts.mockResolvedValue([]);
    mocks.getWorkflowEngineStatus.mockResolvedValue({
      protocol: "rice.workflow.v1",
      available: true,
      runningProjects: 1,
      message: "ready",
    });

    render(
      <WorkflowWorkspace onOpenQa={vi.fn()} qaAvailable onBridgeArtifact={vi.fn()} />,
    );

    const runButton = await screen.findByTitle("打开对应科研任务");
    fireEvent.click(screen.getByRole("button", { name: "确定性 PCA" }));
    expect(screen.getByText("表达矩阵主成分分析")).toBeInTheDocument();

    fireEvent.click(runButton);

    expect(screen.getByRole("button", { name: "本地科研助手" })).toHaveClass("active");
    expect(screen.getByTestId("workflow-agent-panel")).toHaveAttribute(
      "data-focused-run",
      RUN.id,
    );
  });
});
