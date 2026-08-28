// @vitest-environment jsdom

import { describe, expect, it } from "vitest";
import { createElement } from "react";
import { extractCodeBlock, isHtmlPreviewLanguage } from "./html-preview";
import { sanitizeModelHtml } from "./html-sanitize";

describe("sanitizeModelHtml", () => {
  it("keeps presentational html with inline styles", () => {
    const html = '<div style="color:#333"><ol><li><b>样本设计</b></li></ol></div>';
    const out = sanitizeModelHtml(html);
    expect(out).toContain("style=");
    expect(out).toContain("<li>");
    expect(out).toContain("样本设计");
  });

  it("strips scripts and event handlers", () => {
    const html = '<div onclick="x()">ok</div><script>alert(1)</script><img src="x" onerror="y()">';
    const out = sanitizeModelHtml(html);
    expect(out).not.toContain("<script");
    expect(out).not.toContain("onclick");
    expect(out).not.toContain("onerror");
    expect(out).toContain("ok");
  });

  it("strips iframe/embed/form", () => {
    const out = sanitizeModelHtml('<iframe src="https://evil.example"></iframe><form></form>');
    expect(out).not.toContain("iframe");
    expect(out).not.toContain("form");
  });
});

describe("extractCodeBlock", () => {
  it("extracts language and code from a code element", () => {
    const code = createElement("code", { className: "language-html:preview" }, "<div>hi</div>");
    const block = extractCodeBlock(code);
    expect(block).toEqual({ language: "html:preview", code: "<div>hi</div>" });
  });

  it("returns null for plain code elements", () => {
    expect(extractCodeBlock(createElement("code", null, "plain"))).toBeNull();
  });
});

describe("isHtmlPreviewLanguage", () => {
  it("matches html:preview only", () => {
    expect(isHtmlPreviewLanguage("html:preview")).toBe(true);
    expect(isHtmlPreviewLanguage("html")).toBe(false);
    expect(isHtmlPreviewLanguage("html:previewing")).toBe(false);
  });
});
