// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CitationRef, FigureCardAsset, LocatorCandidate } from "../types";
import {
  EvidenceCitations,
  EvidenceFigures,
  EvidenceLocatorCandidates,
  EvidenceWarnings,
} from "./EvidencePlane";

const mocks = vi.hoisted(() => ({
  getFigureAsset: vi.fn(),
}));

vi.mock("../services/tauri-client", () => ({
  getFigureAsset: mocks.getFigureAsset,
}));

const citation: CitationRef = {
  status: "VERIFIED",
  evidenceId: "ev-1",
  fileId: "file-0001",
  filename: "rice.pdf",
  zone: "figure",
  page: 3,
  kbId: "kb-0001",
  revisionId: "rev-0012",
};

const fullFigure: FigureCardAsset = {
  bindingId: "bind-1",
  kbId: "kb-0001",
  fileId: "file-0001",
  revisionId: "rev-0012",
  assetName: "fig-3.png",
  mediaType: "image/png",
  page: 3,
  role: "primary",
  figureLabel: "Figure 3",
};

describe("EvidencePlane", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("渲染引用芯片：文件名 + 区块 + 页码", () => {
    render(<EvidenceCitations citations={[citation]} />);
    expect(screen.getByRole("list", { name: "引用来源" })).toBeInTheDocument();
    expect(screen.getByText(/rice\.pdf/)).toBeInTheDocument();
    expect(screen.getByText(/p\.3/)).toBeInTheDocument();
  });

  it("citations 为空时不渲染", () => {
    const { container } = render(<EvidenceCitations citations={[]} />);
    expect(container.firstChild).toBeNull();
  });

  it("能力位开启且四键齐备时拉取图卡并拼 data URL", async () => {
    mocks.getFigureAsset.mockResolvedValueOnce({
      mediaType: "image/png",
      dataBase64: "aGVsbG8=",
    });
    render(<EvidenceFigures figures={[fullFigure]} capabilityEnabled />);
    await waitFor(() => {
      expect(mocks.getFigureAsset).toHaveBeenCalledWith("kb-0001", "file-0001", "rev-0012", "fig-3.png");
    });
    const img = await screen.findByAltText("Figure 3");
    expect(img).toHaveAttribute("src", "data:image/png;base64,aGVsbG8=");
  });

  it("能力位关闭时绝不发起取图（fail-closed，仅渲染题注）", async () => {
    render(<EvidenceFigures figures={[fullFigure]} capabilityEnabled={false} />);
    // 等一拍确保 effect 不会触发取图
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(mocks.getFigureAsset).not.toHaveBeenCalled();
    expect(screen.getAllByText(/Figure 3/).length).toBeGreaterThan(0);
    expect(screen.getByText(/资产不可用/)).toBeInTheDocument();
  });

  it("缺取图硬前置键（assetName 缺席）时 fail-closed 不取图", async () => {
    const incomplete: FigureCardAsset = { ...fullFigure, assetName: undefined };
    render(<EvidenceFigures figures={[incomplete]} capabilityEnabled />);
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(mocks.getFigureAsset).not.toHaveBeenCalled();
  });

  it("渲染跨文献歧义候选清单", () => {
    const candidates: LocatorCandidate[] = [
      { fileId: "file-a", kbId: "kb-1", filename: "paper-a.pdf" },
      { fileId: "file-b", kbId: "kb-1", filename: "paper-b.pdf" },
    ];
    render(<EvidenceLocatorCandidates candidates={candidates} />);
    expect(screen.getByText(/多个可能指代的文献/)).toBeInTheDocument();
    expect(screen.getByText("paper-a.pdf")).toBeInTheDocument();
    expect(screen.getByText("paper-b.pdf")).toBeInTheDocument();
  });

  it("渲染运行提示条且可逐条关闭", async () => {
    render(<EvidenceWarnings warnings={["部分检索来源超时"]} />);
    expect(screen.getByText("部分检索来源超时")).toBeInTheDocument();
    const dismiss = screen.getByRole("button", { name: "关闭提示" });
    dismiss.click();
    await waitFor(() => {
      expect(screen.queryByText("部分检索来源超时")).not.toBeInTheDocument();
    });
  });
});
