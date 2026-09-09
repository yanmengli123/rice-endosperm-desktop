import { useState } from "react";
import {
  CheckCircle2,
  ChevronDown,
  CircleMinus,
  LoaderCircle,
  XCircle,
} from "lucide-react";
import { buildTraceTimeline, type TraceSpanNode, type TraceState } from "../utils/traceProjection";

function formatDuration(ms: number | null | undefined): string {
  if (ms === null || ms === undefined) return "";
  if (ms < 1000) return `${Math.round(ms)}ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`;
  return `${Math.floor(ms / 60_000)}m${Math.round((ms % 60_000) / 1000)}s`;
}

const DETAIL_ATTR_KEYS = [
  "tool",
  "mcp_server",
  "mcp_tool",
  "args_digest",
  "model_spec",
  "total_tokens",
  "input_tokens",
  "output_tokens",
  "claim_count",
  "evidence_count",
  "wiki_navigation_hit_count",
  "intent",
  "error_code",
  "completeness_status",
];

function SpanNode({ span, depth }: { span: TraceSpanNode; depth: number }) {
  const [expanded, setExpanded] = useState(false);
  const detailEntries = DETAIL_ATTR_KEYS.filter(
    (key) => span.attributes?.[key] !== undefined && span.attributes?.[key] !== null && span.attributes?.[key] !== "",
  ).map((key) => [key, span.attributes[key]] as const);
  const hasDetail = Boolean(span.summary || span.errorType) || detailEntries.length > 0;

  return (
    <div className="trace-span">
      <button
        type="button"
        className={`trace-span-row ${hasDetail ? "is-expandable" : ""}`}
        style={{ paddingLeft: 4 + depth * 12 }}
        onClick={() => hasDetail && setExpanded((value) => !value)}
      >
        <span className="trace-span-status">
          {span.status === "RUNNING" ? (
            <LoaderCircle size={14} className="spin" />
          ) : span.status === "COMPLETED" ? (
            <CheckCircle2 size={14} className="trace-ok" />
          ) : span.status === "FAILED" ? (
            <XCircle size={14} className="trace-err" />
          ) : (
            <CircleMinus size={14} className="trace-warn" />
          )}
        </span>
        <span className={`trace-span-title ${span.status === "FAILED" ? "trace-err" : ""}`}>
          {span.title || span.category}
        </span>
        {span.retryCount > 0 && <span className="trace-span-badge">重试{span.retryCount}</span>}
        {span.durationMs !== null && <span className="trace-span-duration">{formatDuration(span.durationMs)}</span>}
        {hasDetail && <ChevronDown size={13} className={`trace-span-chevron ${expanded ? "is-open" : ""}`} />}
      </button>
      {expanded && hasDetail && (
        <div className="trace-span-detail">
          {span.summary && <div className="trace-span-detail-line">{span.summary}</div>}
          {span.errorType && <div className="trace-span-detail-line trace-err">错误：{span.errorType}</div>}
          {detailEntries.map(([key, value]) => (
            <div key={key} className="trace-span-detail-line">
              <span className="trace-attr-key">{key}</span>
              {Array.isArray(value) ? value.join(", ") : String(value)}
            </div>
          ))}
        </div>
      )}
      {span.children.length > 0 && (
        <div className="trace-span-children">
          {span.children.map((child) => (
            <SpanNode key={child.spanId} span={child} depth={depth + 1} />
          ))}
        </div>
      )}
    </div>
  );
}

/** 本轮执行时间线：真实事件驱动，没有轨迹事件就不渲染（安静原则）。 */
export function TraceTimeline({ trace }: { trace: TraceState }) {
  const timeline = buildTraceTimeline(trace);
  if (timeline.length === 0) return null;
  const summary = trace.summary;
  const parts: string[] = [];
  if (summary?.status) {
    const label: Record<string, string> = {
      running: "进行中",
      completed: "已完成",
      failed: "失败",
      cancelled: "已取消",
      interrupted: "已中断",
    };
    if (label[summary.status]) parts.push(label[summary.status]);
  }
  if (summary?.durationMs != null) parts.push(formatDuration(summary.durationMs));
  if (summary?.ttftMs != null) parts.push(`首字 ${formatDuration(summary.ttftMs)}`);
  if (summary?.totalTokens) parts.push(`${summary.totalTokens.toLocaleString()} tokens`);

  return (
    <section className="trace-timeline" aria-label="本轮执行轨迹">
      {parts.length > 0 && <div className="trace-timeline-summary">{parts.join(" · ")}</div>}
      <div className="trace-timeline-list">
        {timeline.map((span) => (
          <SpanNode key={span.spanId} span={span} depth={0} />
        ))}
      </div>
    </section>
  );
}
