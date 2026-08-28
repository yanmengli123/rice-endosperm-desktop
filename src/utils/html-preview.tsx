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
  return language === HTML_PREVIEW_LANGUAGE;
}

/**
 * html:preview 代码块的沙箱预览：iframe sandbox 为空（禁脚本、禁同源），
 * 内容先经 DOMPurify 净化，与 Web 端预览行为对齐。
 */
export function HtmlPreviewFrame({ html }: { html: string }) {
  const safeHtml = sanitizeModelHtml(html);
  return (
    <div className="html-preview-render">
      <iframe
        className="html-preview-frame"
        sandbox=""
        srcDoc={safeHtml}
        title="HTML 预览"
        loading="lazy"
      />
    </div>
  );
}
