import { describe, expect, it } from "vitest";
import { normalizeMarkdownTables } from "./markdown-tables";

describe("normalizeMarkdownTables", () => {
  it("repairs mismatched scientific table columns", () => {
    const source = `证据类型与限制 | 关系 | 证据等级 | 关系组 | 证据类型 |
|—|—|—|—|
| OsbZIP58 → grain filling | E3 | ASSOCIATION_OR_CONTEXT | 文献观点/关联 | IND-2 热胁迫 |

- 结论需核验。`;

    expect(normalizeMarkdownTables(source)).toBe(`| 证据类型与限制 | 关系 | 证据等级 | 关系组 | 证据类型 |
| --- | --- | --- | --- | --- |
| OsbZIP58 → grain filling | E3 | ASSOCIATION_OR_CONTEXT | 文献观点/关联 | IND-2 热胁迫 |

- 结论需核验。`);
  });

  it("leaves fenced code and ordinary pipe prose unchanged", () => {
    const source = `A | B 是普通文字。

\`\`\`markdown
A | B
— | —
1 | 2
\`\`\`
`;
    expect(normalizeMarkdownTables(source)).toBe(source);
  });

  it("preserves escaped pipes and inline code", () => {
    const source = "| 表达式 | 说明 |\n| - | - |\n| `A | B` | A \\| B |";
    expect(normalizeMarkdownTables(source)).toBe(
      "| 表达式 | 说明 |\n| --- | --- |\n| `A | B` | A \\| B |",
    );
  });
});
