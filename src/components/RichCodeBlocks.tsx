import { useEffect, useId, useState } from "react";
import type { SyntaxHighlighterProps } from "@assistant-ui/react-markdown";
import { Check, Copy, RefreshCw, TriangleAlert } from "lucide-react";
import { Highlight, themes, type Language } from "prism-react-renderer";
import { HtmlPreviewFrame } from "../utils/html-preview";
import { sanitizeModelHtml } from "../utils/html-sanitize";

export function CodeHeader({ language, code }: { language?: string; code: string }) {
  const [copied, setCopied] = useState(false);
  const label = language && language !== "unknown" ? language : "text";
  return (
    <div className="code-block-header">
      <span>{label}</span>
      <button
        type="button"
        onClick={() => {
          void navigator.clipboard.writeText(code).then(() => {
            setCopied(true);
            window.setTimeout(() => setCopied(false), 1400);
          });
        }}
        aria-label="复制代码"
      >
        {copied ? <Check size={14} /> : <Copy size={14} />}
        {copied ? "已复制" : "复制"}
      </button>
    </div>
  );
}

export function PrismCodeBlock({ language, code }: SyntaxHighlighterProps) {
  return (
    <Highlight theme={themes.nightOwlLight} code={code.replace(/\n$/, "")} language={language as Language}>
      {({ className, style, tokens, getLineProps, getTokenProps }) => (
        <pre className={`${className} syntax-code-block`} style={style}>
          <code>
            {tokens.map((line, lineIndex) => (
              <span key={lineIndex} {...getLineProps({ line })} className="syntax-code-line">
                <span className="syntax-line-number" aria-hidden="true">{lineIndex + 1}</span>
                <span>
                  {line.map((token, tokenIndex) => (
                    <span key={tokenIndex} {...getTokenProps({ token })} />
                  ))}
                </span>
              </span>
            ))}
          </code>
        </pre>
      )}
    </Highlight>
  );
}

export function HtmlPreviewCodeBlock({ code }: SyntaxHighlighterProps) {
  return <HtmlPreviewFrame html={code} />;
}

export function MermaidDiagram({ code }: SyntaxHighlighterProps) {
  const reactId = useId();
  const [svg, setSvg] = useState("");
  const [error, setError] = useState("");
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    let cancelled = false;
    const id = `mermaid-${reactId.replace(/[^a-zA-Z0-9_-]/g, "")}-${attempt}`;
    setSvg("");
    setError("");
    void import("mermaid")
      .then(async ({ default: mermaid }) => {
        mermaid.initialize({
          startOnLoad: false,
          securityLevel: "strict",
          theme: "neutral",
          fontFamily: "Inter, 'Segoe UI', sans-serif",
          suppressErrorRendering: true,
        });
        const rendered = await mermaid.render(id, code);
        if (!cancelled) setSvg(sanitizeModelHtml(rendered.svg));
      })
      .catch((reason: unknown) => {
        if (!cancelled) {
          setError(reason instanceof Error ? reason.message : "Mermaid 图表语法无法解析");
        }
      });
    return () => {
      cancelled = true;
    };
  }, [attempt, code, reactId]);

  if (error) {
    return (
      <div className="mermaid-error" role="alert">
        <TriangleAlert size={17} />
        <div><strong>图表渲染失败</strong><span>{error}</span></div>
        <button type="button" onClick={() => setAttempt((value) => value + 1)}><RefreshCw size={14} />重试</button>
      </div>
    );
  }
  if (!svg) return <div className="mermaid-loading"><RefreshCw className="spin" size={16} />正在生成图表…</div>;
  return <div className="mermaid-diagram" dangerouslySetInnerHTML={{ __html: svg }} />;
}
