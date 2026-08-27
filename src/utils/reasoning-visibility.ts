const TAG_PATTERN =
  /\\*(?:(<|&lt;|&#0*60;|&#x0*3c;)\s*(\/\s*)?think\s*(>|&gt;|&#0*62;|&#x0*3e;))/gi;

/**
 * Pure stateless stripper for a complete message: only complete tags are
 * removed, an unmatched opening tag hides the remainder (fail closed), and a
 * incomplete tag fragments are preserved because this function also processes
 * completed history. Live-stream holdback is handled by the Rust buffer.
 */
export function sanitizeVisibleModelText(value: unknown): string {
  const text = typeof value === "string" ? value : "";
  if (!text) return "";

  const visible: string[] = [];
  let cursor = 0;
  let depth = 0;
  TAG_PATTERN.lastIndex = 0;
  for (const match of text.matchAll(TAG_PATTERN)) {
    if (!depth) visible.push(text.slice(cursor, match.index));
    if (match[2]) depth = Math.max(0, depth - 1);
    else depth += 1;
    cursor = match.index + match[0].length;
  }
  if (!depth) visible.push(text.slice(cursor));
  return visible.join("");
}
