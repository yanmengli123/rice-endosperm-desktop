const FENCE_RE = /^ {0,3}(`{3,}|~{3,})/;
const SEPARATOR_CELL_RE = /^:?\s*(?:-+|–+|—+)\s*:?$/;

function fencedLineMask(lines: string[]): boolean[] {
  const masked = Array<boolean>(lines.length).fill(false);
  let activeCharacter = "";
  let activeLength = 0;

  lines.forEach((line, index) => {
    const match = line.match(FENCE_RE);
    if (!activeCharacter) {
      if (match) {
        activeCharacter = match[1][0];
        activeLength = match[1].length;
        masked[index] = true;
      }
      return;
    }

    masked[index] = true;
    if (
      match &&
      match[1][0] === activeCharacter &&
      match[1].length >= activeLength &&
      !line.slice(match[0].length).trim()
    ) {
      activeCharacter = "";
      activeLength = 0;
    }
  });
  return masked;
}

function splitRow(line: string): string[] {
  const text = String(line || "").trim();
  const cells: string[] = [];
  let current = "";
  let inlineMarkerLength = 0;

  for (let index = 0; index < text.length; index += 1) {
    const character = text[index];
    if (character === "\\" && ["|", "｜"].includes(text[index + 1])) {
      current += character + text[index + 1];
      index += 1;
      continue;
    }
    if (character === "`") {
      let end = index + 1;
      while (text[end] === "`") end += 1;
      const markerLength = end - index;
      if (inlineMarkerLength === 0) inlineMarkerLength = markerLength;
      else if (inlineMarkerLength === markerLength) inlineMarkerLength = 0;
      current += text.slice(index, end);
      index = end - 1;
      continue;
    }
    if (["|", "｜"].includes(character) && inlineMarkerLength === 0) {
      cells.push(current.trim());
      current = "";
    } else {
      current += character;
    }
  }

  cells.push(current.trim());
  if (cells[0] === "") cells.shift();
  if (cells[cells.length - 1] === "") cells.pop();
  return cells;
}

function separatorAlignment(cell: string): string | null {
  const value = cell.trim();
  if (!value) return "---";
  if (!SEPARATOR_CELL_RE.test(value)) return null;
  const left = value.startsWith(":");
  const right = value.endsWith(":");
  if (left && right) return ":---:";
  if (right) return "---:";
  if (left) return ":---";
  return "---";
}

function parseSeparator(line: string): string[] | null {
  const cells = splitRow(line);
  if (cells.length < 2 || cells.filter((cell) => cell.trim()).length < 2) return null;
  const normalized = cells.map(separatorAlignment);
  return normalized.some((cell) => cell === null) ? null : (normalized as string[]);
}

function renderRow(cells: string[], width: number, fill = ""): string {
  const padded = cells.slice(0, width);
  while (padded.length < width) padded.push(fill);
  return `| ${padded.join(" | ")} |`;
}

/**
 * Repairs only unmistakable GFM table blocks. This is presentation hardening:
 * cell values are preserved and no scientific content is inferred.
 */
export function normalizeMarkdownTables(content: string): string {
  const source = String(content || "");
  if (!source.includes("|") && !source.includes("｜")) return source;

  const newline = source.includes("\r\n") ? "\r\n" : "\n";
  const trailingNewline = /[\r\n]$/.test(source);
  const lines = source.split(/\r?\n/);
  if (trailingNewline) lines.pop();
  const fenced = fencedLineMask(lines);

  for (let index = 1; index < lines.length; ) {
    if (fenced[index] || fenced[index - 1]) {
      index += 1;
      continue;
    }
    const separator = parseSeparator(lines[index]);
    const header = separator ? splitRow(lines[index - 1]) : [];
    if (!separator || header.length < 2 || lines[index - 1].startsWith("    ")) {
      index += 1;
      continue;
    }

    const bodyRows: string[][] = [];
    let bodyEnd = index + 1;
    while (bodyEnd < lines.length && !fenced[bodyEnd]) {
      const candidate = lines[bodyEnd];
      if (!candidate.trim() || candidate.startsWith("    ")) break;
      const cells = splitRow(candidate);
      if (cells.length < 2) break;
      bodyRows.push(cells);
      bodyEnd += 1;
    }

    const width = Math.max(2, header.length, separator.length, ...bodyRows.map((row) => row.length));
    lines[index - 1] = renderRow(header, width);
    lines[index] = renderRow(separator, width, "---");
    bodyRows.forEach((row, offset) => {
      lines[index + 1 + offset] = renderRow(row, width);
    });
    index = bodyEnd;
  }

  return lines.join(newline) + (trailingNewline ? newline : "");
}
