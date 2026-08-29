import { Children, isValidElement, type ReactNode } from "react";
import { sanitizeModelHtml } from "./html-sanitize";

/**
 * 从 react-markdown 的 <pre> 子节点中提取代码块语言与源码文本。
 * 结构固定为 <pre><code className="language-xxx">text</code></pre>。
 */
export function extractCodeBlock(children: ReactNode): {
  language: string;
  code: string;
} | null {
  const child = Children.only(children) as { props?: { className?: string; children?: ReactNode } };
  if (!isValidElement(child) || !child.props) return null;
  const className = child.props.className || "";
  const match = className.match(/language-([\w:-]+)/);
  if (!match) return null;
  const language = match[1].toLowerCase();
  const code = String(child.props.children ?? "");
  return { language, code };
}

export const HTML_PREVIEW_LANGUAGE = "html:preview";

export function isHtmlPreviewLanguage(language: string): boolean {
  return language === HTML_PREVIEW_LANGUAGE || language === "htmlpreview";
}

/**
 * html:preview 代码块的沙箱预览：iframe sandbox 为空（禁脚本、禁同源），
 * 内容先经 DOMPurify 净化，与 Web 端预览行为对齐。
 */
export function HtmlPreviewFrame({ html }: { html: string }) {
  const safeHtml = sanitizeModelHtml(html);
  const estimatedHeight = Math.min(520, Math.max(120, 76 + html.split("\n").length * 18));
  const srcDoc = `<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; img-src data: blob:; font-src data:"><meta name="viewport" content="width=device-width,initial-scale=1"><style>html,body{margin:0;padding:0;background:transparent;color:#17231d;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;font-size:14px;line-height:1.6;overflow:auto}body{padding:8px;box-sizing:border-box}*{box-sizing:border-box}table{width:100%;border-collapse:collapse}th,td{padding:7px 9px;border:1px solid #d8e3dc;text-align:left}img{max-width:100%;height:auto}</style></head><body>${safeHtml}</body></html>`;
  return (
    <div className="html-preview-render">
      <iframe
        className="html-preview-frame"
        sandbox=""
        srcDoc={srcDoc}
        title="HTML 预览"
        loading="lazy"
        style={{ height: `${estimatedHeight}px` }}
      />
    </div>
  );
}
