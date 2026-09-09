import type { TraceEvent, TraceSnapshot, TraceSpan, TraceSummary } from "../types";

/**
 * 执行轨迹前端投影（桌面端）：yuxi.run-trace.v1 wire 事件 → span/summary 展示状态。
 * 规则与 Web 端 traceProjection.js 及服务端 projector 保持一致：
 * 事件是不可变事实，span 状态由事件序列推导，sequence 单调去重。
 */

export type TraceSpanState = {
  spanId: string;
  parentSpanId: string | null;
  category: string;
  title: string;
  summary: string;
  status: "RUNNING" | "COMPLETED" | "FAILED" | "INTERRUPTED" | "SKIPPED";
  startedAt: number | null;
  durationMs: number | null;
  errorType: string | null;
  retryCount: number;
  attributes: Record<string, unknown>;
};

export type TraceState = {
  runId: string | null;
  lastAppliedSequence: number;
  scannedThroughSequence: number;
  projectionSequence: number;
  spans: Record<string, TraceSpanState>;
  summary: {
    status: string | null;
    ttftMs: number | null;
    durationMs: number | null;
    totalTokens: number | null;
    modelCalls: number;
    toolCalls: number;
    mcpCalls: number;
    knowledgeCalls: number;
    subagentCalls: number;
    retryCount: number;
    errorCount: number;
  } | null;
};

const TERMINAL_SUFFIX_STATUS: Record<string, TraceSpanState["status"]> = {
  completed: "COMPLETED",
  failed: "FAILED",
  interrupted: "INTERRUPTED",
  cancelled: "INTERRUPTED",
  skipped: "SKIPPED",
};

const CATEGORY_COUNT_FIELDS: Record<string, "modelCalls" | "toolCalls" | "mcpCalls" | "knowledgeCalls" | "subagentCalls"> = {
  MODEL: "modelCalls",
  TOOL: "toolCalls",
  MCP: "mcpCalls",
  KNOWLEDGE: "knowledgeCalls",
  SUBAGENT: "subagentCalls",
};

export const createTraceState = (): TraceState => ({
  runId: null,
  lastAppliedSequence: 0,
  scannedThroughSequence: 0,
  projectionSequence: 0,
  spans: {},
  summary: null,
});

/** Resolve the newest persisted server run from desktop-local assistant messages. */
export function findLatestTraceRunId(
  messages: readonly { id?: string; role?: string }[],
): string | null {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const message = messages[index];
    if (message?.role !== "assistant" || typeof message.id !== "string") continue;
    if (!message.id.startsWith("assistant-")) continue;
    const runId = message.id.slice("assistant-".length).trim();
    if (runId) return runId;
  }
  return null;
}

const parseTs = (value?: string | null): number | null => {
  if (!value) return null;
  const parsed = Date.parse(value);
  return Number.isNaN(parsed) ? null : parsed;
};

/** 幂等应用一条 wire 事件；sequence 不前进时返回 false。 */
export function applyTraceEvent(state: TraceState, event: TraceEvent): boolean {
  if (!event || typeof event !== "object") return false;
  if (event.visibility === "ADMIN") return false;
  const sequence = Number(event.sequence) || 0;
  if (sequence <= 0 || sequence <= state.lastAppliedSequence) return false;
  state.lastAppliedSequence = sequence;

  const summary = (state.summary ??= {
    status: null,
    ttftMs: null,
    durationMs: null,
    totalTokens: null,
    modelCalls: 0,
    toolCalls: 0,
    mcpCalls: 0,
    knowledgeCalls: 0,
    subagentCalls: 0,
    retryCount: 0,
    errorCount: 0,
  });
  const occurredAt = parseTs(event.occurred_at);
  const attributes = (event.attributes ?? {}) as Record<string, number | string>;
  const category = String(event.category ?? "");
  const parts = String(event.event_type ?? "").split(".");
  const suffix = parts.length >= 3 ? parts[parts.length - 1] : "";

  if (suffix === "retrying") summary.retryCount += 1;
  if (suffix === "failed") summary.errorCount += 1;

  if (category === "RUN") {
    if (event.event_type === "run.execution.started") summary.status = "running";
    else if (event.event_type.startsWith("run.execution.") && TERMINAL_SUFFIX_STATUS[suffix]) {
      summary.status = suffix;
    }
  }
  const runStart = Object.values(state.spans).find((span) => span.category === "RUN")?.startedAt ?? null;
  if (occurredAt !== null && runStart !== null) {
    summary.durationMs = Math.max(0, occurredAt - runStart);
  }
  const countField = CATEGORY_COUNT_FIELDS[category];
  if (countField && suffix === "started") summary[countField] += 1;
  if (
    category === "MODEL"
    && ["model.generation.first_token", "model.generation.first_visible_token"].includes(event.event_type)
    && summary.ttftMs === null
  ) {
    if (runStart !== null && occurredAt !== null) summary.ttftMs = occurredAt - runStart;
  }

  const spanId = event.span_id;
  if (!spanId) return true;
  let span = state.spans[spanId];
  if (!span) {
    span = state.spans[spanId] = {
      spanId,
      parentSpanId: event.parent_span_id ?? null,
      category,
      title: event.title || category,
      summary: event.summary || "",
      status: "RUNNING",
      startedAt: occurredAt,
      durationMs: null,
      errorType: null,
      retryCount: 0,
      attributes: {},
    };
  } else {
    if (event.title) span.title = event.title;
    if (event.summary) span.summary = event.summary;
    if (event.parent_span_id && !span.parentSpanId) span.parentSpanId = event.parent_span_id;
  }
  if (TERMINAL_SUFFIX_STATUS[suffix]) {
    span.status = TERMINAL_SUFFIX_STATUS[suffix];
    span.durationMs = Number(event.duration_ms) || (span.startedAt !== null && occurredAt !== null ? occurredAt - span.startedAt : 0);
    if (suffix === "failed" || suffix === "interrupted") {
      span.errorType = String(attributes.error_type ?? attributes["error.type"] ?? "") || null;
    }
  } else if (suffix === "retrying") {
    span.retryCount += 1;
    span.status = "RUNNING";
  }
  Object.assign(span.attributes, attributes);
  return true;
}

/** 用服务端快照（权威投影）重置本地状态。 */
export function applyTraceSnapshot(state: TraceState, snapshot: TraceSnapshot): boolean {
  if (state.runId && snapshot.run_id && state.runId !== snapshot.run_id) return false;
  const projectionSequence = Number(snapshot.projection_sequence) || 0;
  if (projectionSequence < state.lastAppliedSequence) return false;
  state.runId = snapshot.run_id ?? state.runId;
  const spans: Record<string, TraceSpanState> = {};
  for (const span of snapshot.spans ?? []) {
    if (!span?.span_id) continue;
    spans[span.span_id] = {
      spanId: span.span_id,
      parentSpanId: span.parent_span_id ?? null,
      category: span.category,
      title: span.title || span.category,
      summary: span.summary || "",
      status: (span.status as TraceSpanState["status"]) ?? "RUNNING",
      startedAt: parseTs(span.started_at),
      durationMs: span.duration_ms ?? null,
      errorType: span.error_type ?? null,
      retryCount: span.retry_count ?? 0,
      attributes: span.attributes ?? {},
    };
  }
  state.spans = spans;
  const s = snapshot.summary;
  state.summary = s
    ? {
        status: s.status ?? null,
        ttftMs: s.ttft_ms ?? null,
        durationMs: s.duration_ms ?? null,
        totalTokens: s.total_tokens ?? null,
        modelCalls: s.model_calls ?? 0,
        toolCalls: s.tool_calls ?? 0,
        mcpCalls: s.mcp_calls ?? 0,
        knowledgeCalls: s.knowledge_calls ?? 0,
        subagentCalls: s.subagent_calls ?? 0,
        retryCount: s.retry_count ?? 0,
        errorCount: s.error_count ?? 0,
      }
    : null;
  state.projectionSequence = projectionSequence;
  state.lastAppliedSequence = projectionSequence;
  state.scannedThroughSequence = Math.max(state.scannedThroughSequence, projectionSequence);
  return true;
}

/** 展示树节点：span + 挂载后的子节点（叶子也带空数组）。 */
export type TraceSpanNode = TraceSpanState & { children: TraceSpanNode[] };

/** 展示树：根 span 在前，children 按 parentSpanId 挂载。 */
export function buildTraceTimeline(state: TraceState): TraceSpanNode[] {
  const list = Object.values(state.spans).sort(
    (a, b) => (a.startedAt ?? 0) - (b.startedAt ?? 0),
  );
  const byId = new Map(list.map((span) => [span.spanId, span]));
  const childrenByParent = new Map<string, TraceSpanState[]>();
  for (const span of list) {
    if (!span.parentSpanId || !byId.has(span.parentSpanId)) continue;
    const children = childrenByParent.get(span.parentSpanId) ?? [];
    children.push(span);
    childrenByParent.set(span.parentSpanId, children);
  }
  const visited = new Set<string>();
  const attach = (span: TraceSpanState, ancestry = new Set<string>()): TraceSpanNode => {
    if (ancestry.has(span.spanId)) return { ...span, children: [] };
    visited.add(span.spanId);
    const next = new Set(ancestry);
    next.add(span.spanId);
    return {
      ...span,
      children: (childrenByParent.get(span.spanId) ?? []).map((child) => attach(child, next)),
    };
  };
  const timeline = list
    .filter((span) => !span.parentSpanId || !byId.has(span.parentSpanId))
    .map((span) => attach(span));
  for (const span of list) {
    if (!visited.has(span.spanId)) timeline.push(attach(span));
  }
  return timeline;
}

export type { TraceSpan, TraceSummary };
