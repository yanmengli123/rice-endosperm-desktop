import { useEffect, useState } from "react";
import { AlertTriangle, FileText, ImageOff, Network, Quote } from "lucide-react";
import { getFigureAsset } from "../services/tauri-client";
import type {
  CitationRef,
  FigureCardAsset,
  FigureRef,
  GraphSnapshot,
  LocatorCandidate,
  PublishableTable,
} from "../types";

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

/** 图表锚点芯片列表（citation_ready v3 figure_refs，ADR-0008）：后端签发的
 * 正文〔图表F#〕锚点绑定，只按结构化载荷展示（不解析芯片文本）。
 * capabilityEnabled=false（服务端未声明 figure_refs 能力位）时不渲染（fail-closed）。 */
export function EvidenceFigureRefs({
  refs,
  capabilityEnabled,
}: {
  refs: FigureRef[];
  capabilityEnabled: boolean;
}) {
  if (!capabilityEnabled || !refs || refs.length === 0) return null;
  const displayable = refs.filter((ref) => ref.ref && ref.label);
  if (displayable.length === 0) return null;
  return (
    <div className="evidence-figure-refs" role="list" aria-label="图表引用锚点">
      {displayable.map((ref) => {
        const parts: string[] = [];
        if (typeof ref.page === "number") parts.push(`p.${ref.page}`);
        if (ref.suppressedReason) parts.push("原图暂不可用");
        const meta = parts.length > 0 ? `（${parts.join(" · ")}）` : "";
        return (
          <span
            key={ref.ref}
            className="evidence-citation-chip"
            title={`${ref.kind === "table" ? "表格" : "图"}引用 · ${ref.label}`}
          >
            {ref.kind === "table" ? (
              <FileText size={12} aria-hidden />
            ) : (
              <ImageOff size={12} aria-hidden />
            )}
            <span>
              {ref.label}
              {meta}
            </span>
          </span>
        );
      })}
    </div>
  );
}

/** 表格卡片（citation_ready v4 tables[]，ADR-0008 P2）：受控渲染的行列 JSON——
 * 单元格只做文本插值（React 自动转义），rowspan/colspan 为数值绑定；
 * 数据来自服务端白名单解析，永不携带原始 HTML。
 * capabilityEnabled=false（服务端未声明 table_cards 能力位）时不渲染（fail-closed）。 */
const safeTableSpan = (value?: number) => {
  if (!Number.isInteger(value)) return 1;
  return Math.max(1, Math.min(value as number, 50));
};

export function EvidenceTables({
  tables,
  capabilityEnabled,
}: {
  tables: PublishableTable[];
  capabilityEnabled: boolean;
}) {
  if (!capabilityEnabled) return null;
  const displayable = (tables ?? []).filter(
    (table) => (table.tableId ?? "").trim() && (table.rows ?? []).length > 0,
  );
  if (displayable.length === 0) return null;
  return (
    <div className="evidence-tables" aria-label="表格卡片">
      {displayable.map((table) => {
        // 本地历史数据也可能来自旧版或被手工篡改；UI 边界重复执行与服务端
        // 相同的行/列/span 护栏，不能把协议字段直接交给 DOM 布局引擎。
        const rows = (table.rows ?? []).slice(0, 200).map((row) => row.slice(0, 60));
        const headerCount = table.headerRows ?? 0;
        const headerRows = headerCount > 0 ? rows.slice(0, headerCount) : [];
        const bodyRows = headerCount > 0 ? rows.slice(headerCount) : rows;
        const label = (table.label ?? "").trim() || (table.page ? `表 · 第${table.page}页` : "表格");
        return (
          <figure className="evidence-table" key={table.tableId} aria-label={label}>
            <figcaption className="evidence-table-caption">
              <strong>{label}</strong>
              <span className="evidence-table-meta">
                {table.rowCount ?? rows.length} 行 × {table.colCount ?? ""} 列
                {table.page ? ` · 第 ${table.page} 页` : ""}
                {table.limited
                  ? " · 已按规模截断"
                  : table.truncated
                    ? " · 跨页已截取"
                    : ""}
              </span>
            </figcaption>
            <div className="evidence-table-scroll">
              <table>
                {headerRows.length > 0 && (
                  <thead>
                    {headerRows.map((row, rowIndex) => (
                      <tr key={`h${rowIndex}`}>
                        {row.map((cell, cellIndex) => (
                          <th
                            key={`h${rowIndex}-${cellIndex}`}
                            rowSpan={safeTableSpan(cell.rowspan) > 1 ? safeTableSpan(cell.rowspan) : undefined}
                            colSpan={safeTableSpan(cell.colspan) > 1 ? safeTableSpan(cell.colspan) : undefined}
                          >
                            {cell.text}
                          </th>
                        ))}
                      </tr>
                    ))}
                  </thead>
                )}
                <tbody>
                  {bodyRows.map((row, rowIndex) => (
                    <tr key={`b${rowIndex}`}>
                      {row.map((cell, cellIndex) => (
                        <td
                          key={`b${rowIndex}-${cellIndex}`}
                          rowSpan={safeTableSpan(cell.rowspan) > 1 ? safeTableSpan(cell.rowspan) : undefined}
                          colSpan={safeTableSpan(cell.colspan) > 1 ? safeTableSpan(cell.colspan) : undefined}
                        >
                          {cell.text}
                        </td>
                      ))}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </figure>
        );
      })}
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
        const knowledgeBaseIds = new Set([
          ...snapshot.nodes.map((node) => node.kb_id),
          ...snapshot.edges.map((edge) => edge.kb_id),
        ]);
        const pendingCount = snapshot.edges.reduce((count, edge) => {
          if (edge.candidate_parallel_count != null) return count + edge.candidate_parallel_count;
          return count + (edge.review_status?.toUpperCase() === "CANDIDATE" ? 1 : 0);
        }, 0);
        return (
          <section
            className="evidence-graph-card"
            key={snapshot.retrieval_id ?? snapshotIndex}
            aria-label={pendingCount ? "已审核及待审核候选关系子图" : "已审核关系子图"}
          >
            <header className="evidence-graph-header">
              <Network size={15} aria-hidden />
              <strong>知识关系图</strong>
              <span>{snapshot.nodes.length} 节点 · {snapshot.edges.length} 关系</span>
              {pendingCount > 0 && (
                <span className="evidence-graph-badge pending">{pendingCount} 条待审核</span>
              )}
              {snapshot.truncated && <span className="evidence-graph-badge">已截断</span>}
              {snapshot.conflict_present && <span className="evidence-graph-badge warning">有冲突</span>}
            </header>
            {snapshot.edges.length > 0 ? (
              <ul className="evidence-graph-relations">
                {snapshot.edges.map((edge) => (
                  <li key={edge.triple_id}>
                    <div>
                      <span>{names.get(edge.source_entity_id) ?? edge.source_entity_id}</span>
                      <b>—{edge.predicate}→</b>
                      <span>{names.get(edge.target_entity_id) ?? edge.target_entity_id}</span>
                    </div>
                    <div className="evidence-graph-relation-meta">
                      {(edge.parallel_count ?? 0) > 1 && <span>×{edge.parallel_count}</span>}
                      {knowledgeBaseIds.size > 1 && <span title={edge.kb_id}>来源 {edge.kb_id}</span>}
                      {edge.review_status?.toUpperCase() === "CANDIDATE" ? (
                        <span className="pending">待审核</span>
                      ) : (edge.candidate_parallel_count ?? 0) > 0 ? (
                        <span className="pending">含 {edge.candidate_parallel_count} 条待审核</span>
                      ) : null}
                      {edge.conflict_status && edge.conflict_status !== "NONE" && (
                        <span className="warning">冲突</span>
                      )}
                    </div>
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
