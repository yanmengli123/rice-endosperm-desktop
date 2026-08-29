import { normalizeMathDelimiters } from "@assistant-ui/react-markdown";
import { normalizeMarkdownTables } from "./markdown-tables";

const HTML_BLOCK_TAGS = new Set([
  "article",
  "aside",
  "details",
  "div",
  "figure",
  "section",
  "table",
]);

function fenceFor(source: string): string {
  const longest = Math.max(0, ...Array.from(source.matchAll(/`+/g), (match) => match[0].length));
  return "`".repeat(Math.max(3, longest + 1));
}

function unescapeHtmlLine(line: string): string {
  return line.replace(/\\([<>])/g, "$1");
}

function rootTagOf(line: string): string | undefined {
  const match = line.trimStart().match(/^\\?<([a-z][\w-]*)\b/i);
  const tag = match?.[1]?.toLowerCase();
  return tag && HTML_BLOCK_TAGS.has(tag) ? tag : undefined;
}

function tagDelta(line: string, tag: string): number {
  const normalized = unescapeHtmlLine(line);
  const matcher = new RegExp(`<\\/?${tag}\\b[^>]*>`, "gi");
  let delta = 0;
  for (const match of normalized.matchAll(matcher)) {
    const value = match[0];
    if (value.startsWith("</")) delta -= 1;
    else if (!value.endsWith("/>")) delta += 1;
  }
  return delta;
}

/**
 * 模型有时直接输出带行内样式的 HTML 卡片，Markdown 解析器会因转义或
 * 流式半成品而把标签显示成正文。把独立块规范成专用代码语言，后续由
 * sandbox iframe 展示；普通 Markdown、行内 HTML 与代码围栏保持原样。
 */
export function isolateStandaloneHtmlBlocks(markdown: string): string {
  const lines = markdown.replace(/\r\n?/g, "\n").split("\n");
  const output: string[] = [];
  let inFence = false;
  let fenceMarker = "";

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    const fenceMatch = line.match(/^\s*(`{3,}|~{3,})/);
    if (fenceMatch) {
      const marker = fenceMatch[1][0];
      if (!inFence) {
        inFence = true;
        fenceMarker = marker;
      } else if (marker === fenceMarker) {
        inFence = false;
        fenceMarker = "";
      }
      output.push(line);
      continue;
    }

    const rootTag = inFence ? undefined : rootTagOf(line);
    if (!rootTag) {
      output.push(line);
      continue;
    }

    const htmlLines = [unescapeHtmlLine(line)];
    let depth = tagDelta(line, rootTag);
    while (depth > 0 && index + 1 < lines.length) {
      index += 1;
      const nextLine = lines[index];
      htmlLines.push(unescapeHtmlLine(nextLine));
      depth += tagDelta(nextLine, rootTag);
    }

    const html = htmlLines.join("\n");
    const fence = fenceFor(html);
    output.push(`${fence}htmlpreview`, html, fence);
  }

  return output.join("\n");
}

export function normalizeRichAnswer(markdown: string): string {
  return normalizeMathDelimiters(
    normalizeMarkdownTables(isolateStandaloneHtmlBlocks(markdown)),
  );
}
