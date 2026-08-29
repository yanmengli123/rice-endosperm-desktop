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

function PrismSource({ language, code }: { language: string; code: string }) {
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

export function PrismCodeBlock(props: SyntaxHighlighterProps) {
  return <PrismSource language={props.language} code={props.code} />;
}

export function HtmlPreviewCodeBlock({ code }: SyntaxHighlighterProps) {
  return <HtmlPreviewFrame html={code} />;
}

/**
 * ```html 围栏块：科研模型常用带内联样式的 HTML 卡片组织结构化答案
 * （如"调控基因分类卡片"），按源码展示会得到一大块可读性极差的标签文本。
 * 默认走沙箱渲染预览（与 html:preview 同一净化/iframe 策略），
 * 提供"查看源码"切换保留原始代码语义；脚本/事件属性已被 DOMPurify 剥离。
 */
export function HtmlFencedCodeBlock({ code }: { code: string }) {
  const [showSource, setShowSource] = useState(false);
  const [copied, setCopied] = useState(false);
  const source = code.replace(/\n$/, "");
  return (
    <div className="html-fenced-block">
      <div className="code-block-header html-fenced-header">
        <span>html</span>
        <div className="html-fenced-actions">
          <button
            type="button"
            onClick={() => setShowSource((value) => !value)}
            aria-label={showSource ? "切换到渲染视图" : "切换到源码视图"}
          >
            {showSource ? "渲染视图" : "查看源码"}
          </button>
          <button
            type="button"
            onClick={() => {
              void navigator.clipboard.writeText(source).then(() => {
                setCopied(true);
                window.setTimeout(() => setCopied(false), 1400);
              });
            }}
            aria-label="复制代码"
          >
            {copied ? "已复制" : "复制"}
          </button>
        </div>
      </div>
      {showSource ? <PrismSource language="html" code={source} /> : <HtmlPreviewFrame html={source} />}
    </div>
  );
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
