// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CitationRef, FigureCardAsset, GraphSnapshot, LocatorCandidate, PublishableTable } from "../types";
import {
  EvidenceCitations,
  EvidenceFigures,
  EvidenceGraphSnapshots,
  EvidenceLocatorCandidates,
  EvidenceTables,
  EvidenceWarnings,
  FollowupSuggestions,
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

  it("表格卡片按能力位失败关闭，并将文本转义、跨度限制在安全范围", () => {
    const table: PublishableTable = {
      tableId: "tbl-1",
      label: "Table 1",
      page: 11,
      headerRows: 1,
      rows: [
        [{ text: "<script>alert(1)</script>", header: true, rowspan: 999, colspan: 999 }],
        [{ text: "75.2", rowspan: -4, colspan: 0 }],
      ],
      rowCount: 2,
      colCount: 1,
    };

    const closed = render(<EvidenceTables tables={[table]} capabilityEnabled={false} />);
    expect(closed.container.firstChild).toBeNull();
    closed.unmount();

    render(<EvidenceTables tables={[table]} capabilityEnabled />);
    const textCell = screen.getByText("<script>alert(1)</script>");
    expect(textCell.tagName).toBe("TH");
    expect(textCell).toHaveAttribute("rowspan", "50");
    expect(textCell).toHaveAttribute("colspan", "50");
    expect(document.querySelector("script")).toBeNull();
    expect(screen.getByText(/第 11 页/)).toBeInTheDocument();
  });

  it("关系图降级列表披露候选计数、冲突和多知识库来源", () => {
    const snapshot: GraphSnapshot = {
      schema: "graph_snapshot_v1",
      outcome: "HIT",
      nodes: [
        { entity_id: "e1", kb_id: "kb-a", name: "GS3", label: "Gene", is_seed: true },
        { entity_id: "e2", kb_id: "kb-b", name: "grain size", label: "Trait" },
      ],
      edges: [
        {
          triple_id: "t1",
          kb_id: "kb-b",
          source_entity_id: "e1",
          target_entity_id: "e2",
          predicate: "regulates",
          review_status: "APPROVED",
          parallel_count: 3,
          candidate_parallel_count: 2,
          conflict_status: "CONTESTED",
        },
      ],
    };

    render(<EvidenceGraphSnapshots snapshots={[snapshot]} />);
    expect(screen.getByLabelText("已审核及待审核候选关系子图")).toBeInTheDocument();
    expect(screen.getByText("2 条待审核")).toBeInTheDocument();
    expect(screen.getByText("含 2 条待审核")).toBeInTheDocument();
    expect(screen.getByText("来源 kb-b")).toBeInTheDocument();
    expect(screen.getByText("冲突")).toBeInTheDocument();
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

describe("FollowupSuggestions", () => {
  it("空清单不渲染，非空清单点击即回调原样问题", () => {
    const { container, unmount } = render(
      <FollowupSuggestions suggestions={[]} onSelect={() => undefined} />,
    );
    expect(container).toBeEmptyDOMElement();
    unmount();

    const onSelect = vi.fn();
    render(
      <FollowupSuggestions
        suggestions={["灌浆期淀粉积累的关键酶有哪些？", "胚乳细胞何时分化？"]}
        onSelect={onSelect}
      />,
    );
    expect(screen.getByLabelText("推荐的后续问题")).toBeInTheDocument();
    screen.getByRole("button", { name: "胚乳细胞何时分化？" }).click();
    expect(onSelect).toHaveBeenCalledWith("胚乳细胞何时分化？");
  });
});
