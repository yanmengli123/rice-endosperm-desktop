import { describe, expect, it } from "vitest";
import { isolateStandaloneHtmlBlocks, normalizeRichAnswer } from "./rich-answer";

describe("isolateStandaloneHtmlBlocks", () => {
  it("moves a nested styled html card into a sandbox preview fence", () => {
    const source = [
      '<div style="color:#234">',
      "  <div>FLO7<br>OsNF-YB1</div>",
      "</div>",
      "",
      "后续说明",
    ].join("\n");
    const result = isolateStandaloneHtmlBlocks(source);
    expect(result).toContain("```htmlpreview");
    expect(result).toContain('<div style="color:#234">');
    expect(result).toContain("```\n\n后续说明");
  });

  it("repairs model-escaped block tags without touching fenced source code", () => {
    const source = ["\\<div>结果\\</div>", "", "```html", "<div>示例</div>", "```"].join("\n");
    const result = isolateStandaloneHtmlBlocks(source);
    expect(result).toContain("```htmlpreview\n<div>结果</div>\n```");
    expect(result).toContain("```html\n<div>示例</div>\n```");
  });
});

describe("normalizeRichAnswer", () => {
  it("normalizes latex bracket delimiters", () => {
    expect(normalizeRichAnswer("\\(x^2+y^2\\)")).toBe("$x^2+y^2$");
  });
});
