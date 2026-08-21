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
});
