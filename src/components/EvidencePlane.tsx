import { useEffect, useState } from "react";
import { AlertTriangle, FileText, ImageOff, Network, Quote } from "lucide-react";
import { getFigureAsset } from "../services/tauri-client";
import type { CitationRef, FigureCardAsset, GraphSnapshot, LocatorCandidate } from "../types";

/** 单条结构化引用芯片（citation_ready v2）：文件名 + 区块/页码，按实值展示，缺席不猜。 */
function CitationChip({ citation }: { citation: CitationRef }) {
  const parts: string[] = [];
  if (citation.filename) parts.push(citation.filename);
  if (citation.zone) parts.push(citation.zone);
  if (typeof citation.page === "number") parts.push(`p.${citation.page}`);
  const label = parts.length > 0 ? parts.join(" · ") : (citation.evidenceId ?? "引用来源");
  return (
    <span className="evidence-citation-chip" title={citation.status ? `定位状态：${citation.status}` : undefined}>
      <Quote size={12} aria-hidden />
      <span>{label}</span>
    </span>
  );
}

/** 引用芯片列表（一次 run 可携带多条 citation）。 */
export function EvidenceCitations({ citations }: { citations: CitationRef[] }) {
  if (!citations || citations.length === 0) return null;
  return (
    <div className="evidence-citations" role="list" aria-label="引用来源">
      {citations.map((citation, index) => (
        <CitationChip key={citation.evidenceId ?? citation.anchorId ?? index} citation={citation} />
      ))}
    </div>
  );
}

/** 取图硬前置：四键（kbId/fileId/revisionId/assetName）齐备且非空才允许拉取（fail-closed）。 */
function isFetchable(figure: FigureCardAsset): boolean {
  return [figure.kbId, figure.fileId, figure.revisionId, figure.assetName].every(
    (value) => typeof value === "string" && value.trim().length > 0,
  );
}

function FigureCard({ figure, capabilityEnabled }: { figure: FigureCardAsset; capabilityEnabled: boolean }) {
  const [src, setSrc] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  // 能力位驱动：服务端未声明 figures_card 能力位时绝不发起取图（fail-closed）。
  const fetchable = capabilityEnabled && isFetchable(figure);

  useEffect(() => {
    if (!fetchable) return;
    let cancelled = false;
    setSrc(null);
    setFailed(false);
    getFigureAsset(figure.kbId!, figure.fileId!, figure.revisionId!, figure.assetName!)
      .then((payload) => {
        if (cancelled) return;
        // 内存 data URL，随消息生命周期驻留，不落盘。
        setSrc(`data:${payload.mediaType};base64,${payload.dataBase64}`);
      })
      .catch(() => {
        if (!cancelled) setFailed(true);
      });
    return () => {
      cancelled = true;
    };
  }, [fetchable, figure.kbId, figure.fileId, figure.revisionId, figure.assetName]);

  const caption = figure.figureLabel ?? figure.caption ?? figure.panelLabel;
  const metaParts: string[] = [];
  if (typeof figure.page === "number") metaParts.push(`p.${figure.page}`);
  if (figure.panelLabel && figure.panelLabel !== caption) metaParts.push(`面板 ${figure.panelLabel}`);
  const meta = metaParts.length > 0 ? metaParts.join(" · ") : null;

  // 缺取图硬前置键：fail-closed 不渲染图像（仍保留题注，避免静默吞掉证据）。
  if (!fetchable || failed) {
    return (
      <figure className="evidence-figure evidence-figure-unavailable" role="img" aria-label={caption ?? "图卡不可用"}>
        <div className="evidence-figure-placeholder">
          <ImageOff size={18} aria-hidden />
          <span>{caption ?? "图卡"}（资产不可用）</span>
        </div>
        {caption && <figcaption className="evidence-figure-caption">{caption}</figcaption>}
      </figure>
    );
  }
  return (
    <figure className="evidence-figure" role="group" aria-label={caption ?? "图卡"}>
      {src ? (
        <img
          src={src}
          alt={caption ?? "科研图表"}
          width={figure.width}
          height={figure.height}
          loading="lazy"
        />
      ) : (
        <div className="evidence-figure-placeholder" aria-hidden>
          <span className="evidence-figure-loading">图卡加载中…</span>
        </div>
      )}
      {(caption || meta) && (
        <figcaption className="evidence-figure-caption">
          {caption}
          {caption && meta ? " " : ""}
          {meta && <span className="evidence-figure-meta">{meta}</span>}
        </figcaption>
      )}
    </figure>
  );
}

/** 图卡组（role=primary 大图 + panel/其余缩略，对齐 Web FigureCardGroup 语义）。
 * figures 缺席 ⟺ 本 run 未发布图卡：上层不调用本组件。
 * capabilityEnabled=false（服务端未声明 figures_card 能力位）时只渲染题注不取图。 */
export function EvidenceFigures({
  figures,
  capabilityEnabled,
}: {
  figures: FigureCardAsset[];
  capabilityEnabled: boolean;
}) {
  if (!figures || figures.length === 0) return null;
  return (
    <div className="evidence-figures" aria-label="证据图卡">
      {figures.map((figure, index) => (
        <FigureCard
          key={figure.bindingId ?? figure.assetName ?? index}
          figure={figure}
          capabilityEnabled={capabilityEnabled}
        />
      ))}
    </div>
  );
}

/** 跨文献歧义候选清单（locator_candidates）：只展示文档身份；一键重问依赖服务端 mention.v2。 */
export function EvidenceLocatorCandidates({ candidates }: { candidates: LocatorCandidate[] }) {
  if (!candidates || candidates.length === 0) return null;
  return (
    <div className="evidence-locator" role="note" aria-label="可能指代的文献">
      <div className="evidence-locator-title">检测到多个可能指代的文献：</div>
      <ul className="evidence-locator-list">
        {candidates.map((candidate) => (
          <li key={candidate.fileId} className="evidence-locator-item">
            <FileText size={13} aria-hidden />
            <span>{candidate.filename}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

/** 桌面端关系图降级投影：与 Web 画布消费同一 graph_snapshot_v1，
 * 用可访问的节点/边卡片展示，绝不从回答文本重建关系。 */
export function EvidenceGraphSnapshots({ snapshots }: { snapshots: GraphSnapshot[] }) {
  if (!snapshots || snapshots.length === 0) return null;
  return (
    <div className="evidence-graphs" aria-label="规范知识关系图">
      {snapshots.map((snapshot, snapshotIndex) => {
        const names = new Map(snapshot.nodes.map((node) => [node.entity_id, node.name]));
        return (
          <section
            className="evidence-graph-card"
            key={snapshot.retrieval_id ?? snapshotIndex}
            aria-label="已审核关系子图"
          >
            <header className="evidence-graph-header">
              <Network size={15} aria-hidden />
              <strong>知识关系图</strong>
              <span>{snapshot.nodes.length} 节点 · {snapshot.edges.length} 关系</span>
              {snapshot.truncated && <span className="evidence-graph-badge">已截断</span>}
              {snapshot.conflict_present && <span className="evidence-graph-badge warning">有冲突</span>}
            </header>
            {snapshot.edges.length > 0 ? (
              <ul className="evidence-graph-relations">
                {snapshot.edges.map((edge) => (
                  <li key={edge.triple_id}>
                    <span>{names.get(edge.source_entity_id) ?? edge.source_entity_id}</span>
                    <b>—{edge.predicate}→</b>
                    <span>{names.get(edge.target_entity_id) ?? edge.target_entity_id}</span>
                  </li>
                ))}
              </ul>
            ) : (
              <div className="evidence-graph-empty">当前审核策略下无可发布关系</div>
            )}
          </section>
        );
      })}
    </div>
  );
}

/** 运行内非阻塞提示条（yuxi.warning）：可关闭，不影响终态。 */
export function EvidenceWarnings({ warnings }: { warnings: string[] }) {
  const [dismissed, setDismissed] = useState(0);
  const visible = (warnings ?? []).slice(dismissed);
  if (visible.length === 0) return null;
  return (
    <div className="evidence-warnings">
      {visible.map((message, index) => (
        <div key={`${dismissed}-${index}`} className="evidence-warning" role="status">
          <AlertTriangle size={14} aria-hidden />
          <span>{message}</span>
          <button
            type="button"
            className="evidence-warning-dismiss"
            onClick={() => setDismissed((count) => count + 1)}
            aria-label="关闭提示"
          >
            ×
          </button>
        </div>
      ))}
    </div>
  );
}
