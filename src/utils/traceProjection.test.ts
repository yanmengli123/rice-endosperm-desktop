import { describe, expect, it } from "vitest";
import type { TraceEvent } from "../types";
import {
  applyTraceEvent,
  applyTraceSnapshot,
  buildTraceTimeline,
  createTraceState,
  findLatestTraceRunId,
} from "./traceProjection";

const wireEvent = (overrides: Partial<TraceEvent> = {}): TraceEvent => ({
  sequence: 1,
  category: "TOOL",
  operation: "execution",
  event_type: "tool.execution.started",
  span_id: "tool-1",
  occurred_at: "2026-09-08T06:42:00.000Z",
  ...overrides,
});

describe("traceProjection", () => {
  it("restores the latest persisted desktop run from assistant messages", () => {
    expect(findLatestTraceRunId([
      { id: "user-1", role: "user" },
      { id: "assistant-run-old", role: "assistant" },
      { id: "user-2", role: "user" },
      { id: "assistant-run-new", role: "assistant" },
    ])).toBe("run-new");
    expect(findLatestTraceRunId([{ id: "local-note", role: "assistant" }])).toBeNull();
  });

  it("应用事件序列推导 span 状态机（started→retrying→completed）", () => {
    const state = createTraceState();
    expect(applyTraceEvent(state, wireEvent())).toBe(true);
    applyTraceEvent(
      state,
      wireEvent({ sequence: 2, event_type: "tool.execution.retrying", occurred_at: "2026-09-08T06:42:01.000Z" }),
    );
    applyTraceEvent(
      state,
      wireEvent({
        sequence: 3,
        event_type: "tool.execution.completed",
        occurred_at: "2026-09-08T06:42:03.000Z",
        duration_ms: 3000,
        summary: "命中 18 条",
      }),
    );
    expect(state.spans["tool-1"].status).toBe("COMPLETED");
    expect(state.spans["tool-1"].retryCount).toBe(1);
    expect(state.spans["tool-1"].durationMs).toBe(3000);
    expect(state.summary?.toolCalls).toBe(1);
    expect(state.summary?.retryCount).toBe(1);
  });

  it("sequence 不前进的事件幂等丢弃", () => {
    const state = createTraceState();
    applyTraceEvent(state, wireEvent({ sequence: 5 }));
    expect(applyTraceEvent(state, wireEvent({ sequence: 5 }))).toBe(false);
    expect(applyTraceEvent(state, wireEvent({ sequence: 4 }))).toBe(false);
    expect(state.lastAppliedSequence).toBe(5);
    expect(state.summary?.toolCalls).toBe(1);
  });

  it("服务端快照重置状态并抬高去重水位", () => {
    const state = createTraceState();
    state.lastAppliedSequence = 3;
    applyTraceSnapshot(state, {
      run_id: "run-1",
      snapshot_sequence: 10,
      projection_sequence: 10,
      summary: { status: "completed", model_calls: 2 },
      spans: [{ span_id: "run", category: "RUN", status: "COMPLETED" }],
    });
    expect(state.lastAppliedSequence).toBe(10);
    expect(state.spans.run.status).toBe("COMPLETED");
    expect(applyTraceEvent(state, wireEvent({ sequence: 7 }))).toBe(false);
  });

  it("拒绝上一轮迟到的终态快照覆盖当前 run", () => {
    const state = createTraceState();
    state.runId = "run-new";
    state.lastAppliedSequence = 2;
    const applied = applyTraceSnapshot(state, {
      run_id: "run-old",
      snapshot_sequence: 9,
      projection_sequence: 9,
      summary: { status: "completed" },
      spans: [],
    });
    expect(applied).toBe(false);
    expect(state.runId).toBe("run-new");
    expect(state.lastAppliedSequence).toBe(2);
  });

  it("时间线树按 parentSpanId 组装", () => {
    const state = createTraceState();
    applyTraceEvent(state, wireEvent({ sequence: 1, category: "RUN", event_type: "run.execution.started", span_id: "run" }));
    applyTraceEvent(
      state,
      wireEvent({ sequence: 2, category: "KNOWLEDGE", event_type: "knowledge.search.started", span_id: "kr_1", parent_span_id: "run" }),
    );
    const timeline = buildTraceTimeline(state);
    expect(timeline).toHaveLength(1);
    expect(timeline[0].spanId).toBe("run");
    expect(timeline[0].children[0].spanId).toBe("kr_1");
  });
});
