import { describe, expect, it } from "vitest";
import { sanitizeVisibleModelText } from "./reasoning-visibility";

describe("sanitizeVisibleModelText", () => {
  it.each([
    "<think>private chain</think>公开答案",
    "\\\\<THINK>private chain</think>公开答案",
    "&lt;think&gt;private chain&lt;/think&gt;公开答案",
    "&#x3c;think&#x3e;private chain&#x3c;/think&#x3e;公开答案",
  ])("removes provider reasoning from %s", (value) => {
    expect(sanitizeVisibleModelText(value)).toBe("公开答案");
  });

  it("fails closed for an unclosed opening tag", () => {
    expect(sanitizeVisibleModelText("<think>private chain")).toBe("");
  });

  it("keeps ordinary assistant text unchanged", () => {
    expect(sanitizeVisibleModelText("你好！我是稻芯智析。")).toBe("你好！我是稻芯智析。");
  });
});
