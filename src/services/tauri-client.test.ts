import { describe, expect, it } from "vitest";
import { normalizeCommandError } from "./tauri-client";

describe("normalizeCommandError", () => {
  it("preserves safe command error fields", () => {
    const result = normalizeCommandError({
      code: "unauthorized",
      message: "API Key 无效或已停用",
      retryable: false,
    });

    expect(result.message).toBe("API Key 无效或已停用");
    expect(result.code).toBe("unauthorized");
    expect(result.retryable).toBe(false);
  });

  it("does not serialize arbitrary objects into the user message", () => {
    expect(normalizeCommandError({ secret: "yxkey_sensitive" }).message).toBe("发生未知错误");
  });

  it("preserves server guidance action and gateway trace id", () => {
    const result = normalizeCommandError({
      code: "daily_run_quota_exceeded",
      message: "今日问答次数已达上限，请联系管理员",
      retryable: false,
      status: 429,
      action: "contact_admin",
      traceId: "8f14e45f-ea0b-4a1d-9f6c-2a77c1b0e001",
    });

    expect(result.action).toBe("contact_admin");
    expect(result.traceId).toBe("8f14e45f-ea0b-4a1d-9f6c-2a77c1b0e001");
    // Rust 端以 snake_case trace_id 序列化时同样兼容
    const legacy = normalizeCommandError({
      code: "run_busy",
      message: "该会话已有任务在运行",
      retryable: false,
      trace_id: "legacy-shape-id",
    });
    expect(legacy.traceId).toBe("legacy-shape-id");
  });
});
