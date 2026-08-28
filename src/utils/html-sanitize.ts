import DOMPurify from "dompurify";

type PurifyInstance = ReturnType<typeof DOMPurify>;
type PurifyConfig = Parameters<PurifyInstance["sanitize"]>[1];

/**
 * 与 Web 端 renderMarkdown 的 DOMPurify 策略保持一致：
 * 允许模型输出的展示型 HTML（div/span/列表/表格/行内样式等），
 * 剥离脚本、事件属性与危险嵌入。
 */
const PURIFY_CONFIG: PurifyConfig = {
  ADD_TAGS: ["html", "head", "body"],
  ADD_ATTR: [
    "class",
    "style",
    "target",
    "rel",
    "type",
    "checked",
    "disabled",
    "colspan",
    "rowspan",
  ],
  FORBID_TAGS: [
    "script",
    "iframe",
    "object",
    "embed",
    "form",
    "input",
    "textarea",
    "select",
    "base",
    "meta",
    "link",
  ],
  FORBID_ATTR: ["srcdoc", "sandbox", "onerror", "onload", "onclick"],
};

let cachedPurify: PurifyInstance | null = null;

function getPurify(): PurifyInstance | null {
  if (cachedPurify) return cachedPurify;
  // DOMPurify 需要 DOM；在非 DOM 测试环境下返回 null，由调用方降级处理。
  const purify = DOMPurify(window);
  if (!purify || typeof purify.sanitize !== "function" || !purify.isSupported) {
    return null;
  }
  cachedPurify = purify;
  return cachedPurify;
}

/**
 * 净化模型输出的 HTML。DOM 不可用时（纯 Node 测试环境）原样返回，
 * 由调用方决定降级策略；生产环境（Tauri WebView）始终有 DOM。
 */
export function sanitizeModelHtml(html: string): string {
  const purify = getPurify();
  if (!purify) return html;
  return purify.sanitize(html, PURIFY_CONFIG) as unknown as string;
}