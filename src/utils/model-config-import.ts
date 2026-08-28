/**
 * JSON 一键导入的客户端预解析。
 *
 * 与服务端 parse_claude_model_configuration 保持同一套别名规则：
 * - API Key：ANTHROPIC_API_KEY → ANTHROPIC_AUTH_TOKEN（Claude Code 两种标准写法）
 * - 模型名：ANTHROPIC_MODEL → DEFAULT_SONNET(_NAME) → REASONING → DEFAULT_OPUS(_NAME) → DEFAULT_HAIKU(_NAME)
 *
 * 预览只在本地进行，真正的保存与激活仍由服务端权威完成（fail-closed）。
 * 环境变量原文不落盘：仅本次请求内存中存在。
 */

const API_KEY_KEYS = ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"] as const;
const BASE_URL_KEYS = ["ANTHROPIC_BASE_URL"] as const;
const MODEL_KEYS = [
  "ANTHROPIC_MODEL",
  "ANTHROPIC_DEFAULT_SONNET_MODEL",
  "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
  "ANTHROPIC_REASONING_MODEL",
  "ANTHROPIC_DEFAULT_OPUS_MODEL",
  "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
  "ANTHROPIC_DEFAULT_HAIKU_MODEL",
  "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
] as const;

const SUPPORTED_KEYS = new Set<string>([...API_KEY_KEYS, ...BASE_URL_KEYS, ...MODEL_KEYS]);

export type ResolvedImportField = {
  key: string;
  value: string;
};

export type ModelConfigurationPreview = {
  ok: boolean;
  /** 解析失败时的用户可读原因（不回显原始 JSON）。 */
  error?: string;
  protocol: "anthropic";
  baseUrl?: string;
  model?: string;
  maskedApiKey?: string;
  resolvedSources: {
    apiKey?: string;
    baseUrl?: string;
    model?: string;
  };
  resolvedFields: {
    apiKey?: ResolvedImportField;
    baseUrl?: ResolvedImportField;
    model?: ResolvedImportField;
  };
  ignoredFields: string[];
};

function maskApiKey(apiKey: string): string {
  if (apiKey.length <= 10) return `${apiKey.slice(0, 2)}****`;
  return `${apiKey.slice(0, 6)}****${apiKey.slice(-4)}`;
}

function scalar(value: unknown): string | null {
  if (value === null || typeof value === "boolean") return null;
  if (typeof value === "number") return Number.isFinite(value) ? String(value) : null;
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  return trimmed || null;
}

function resolveField(env: Record<string, unknown>, keys: readonly string[]): ResolvedImportField | undefined {
  for (const key of keys) {
    const value = scalar(env[key]);
    if (value) return { key, value };
  }
  return undefined;
}

export function parseModelConfigurationJson(text: string): ModelConfigurationPreview {
  const preview: ModelConfigurationPreview = {
    ok: false,
    protocol: "anthropic",
    resolvedSources: {},
    resolvedFields: {},
    ignoredFields: [],
  };
  const trimmed = text.trim();
  if (!trimmed) return { ...preview, error: "请先粘贴 JSON 配置" };

  let payload: unknown;
  try {
    payload = JSON.parse(trimmed);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    const position = /position (\d+)/.exec(message);
    let detail = message;
    if (position) {
      const index = Number(position[1]);
      const prefix = trimmed.slice(0, index);
      const line = prefix.split("\n").length;
      detail = `第 ${line} 行附近解析失败`;
    }
    return { ...preview, error: `JSON 格式错误：${detail}` };
  }

  if (typeof payload !== "object" || payload === null || Array.isArray(payload)) {
    return { ...preview, error: "配置根节点必须是 JSON 对象" };
  }
  const env = (payload as { env?: unknown }).env;
  if (typeof env !== "object" || env === null || Array.isArray(env)) {
    return { ...preview, error: "JSON 必须包含 env 对象" };
  }
  const envRecord = env as Record<string, unknown>;

  const apiKey = resolveField(envRecord, API_KEY_KEYS);
  const baseUrl = resolveField(envRecord, BASE_URL_KEYS);
  const model = resolveField(envRecord, MODEL_KEYS);

  const missing: string[] = [];
  if (!apiKey) missing.push(`API Key（${API_KEY_KEYS.join(" 或 ")}）`);
  if (!baseUrl) missing.push(`API Base URL（${BASE_URL_KEYS[0]}）`);
  if (!model) missing.push(`模型名（${MODEL_KEYS[0]} 或 DEFAULT_*_MODEL 系列）`);
  if (missing.length) return { ...preview, error: `env 缺少必需字段：${missing.join("；")}` };

  if (!/^https:\/\//i.test(baseUrl!.value)) {
    return { ...preview, error: "API Base URL 必须使用 HTTPS（企业安全策略要求传输加密）" };
  }

  const ignoredFields = Object.keys(envRecord)
    .filter((key) => !SUPPORTED_KEYS.has(key))
    .sort();
  if (Object.prototype.hasOwnProperty.call(payload, "includeCoAuthoredBy")) {
    ignoredFields.push("includeCoAuthoredBy");
  }

  return {
    ok: true,
    protocol: "anthropic",
    baseUrl: baseUrl!.value,
    model: model!.value,
    maskedApiKey: maskApiKey(apiKey!.value),
    resolvedSources: {
      apiKey: apiKey!.key,
      baseUrl: baseUrl!.key,
      model: model!.key,
    },
    resolvedFields: { apiKey, baseUrl, model },
    ignoredFields,
  };
}

/**
 * 发送给服务端前的兼容归一化。
 *
 * 新版 Yuxi 服务端同时接受 ``ANTHROPIC_API_KEY`` 与 ``ANTHROPIC_AUTH_TOKEN``；
 * 但更早版本只认 ``ANTHROPIC_API_KEY``（会报 "env 必须包含 ANTHROPIC_API_KEY、
 * ANTHROPIC_BASE_URL 和 ANTHROPIC_MODEL"）。这里在只提供了 AUTH_TOKEN 时补
 * 一份 API_KEY 副本，让新旧服务端都能导入。其余字段原样保留；API Key 原文
 * 只在本次请求内存中存在，不落盘。
 *
 * 解析失败时原样返回输入，由调用方把后续错误透传给用户。
 */
export function normalizeConfigurationForServer(text: string): string {
  const trimmed = text.trim();
  if (!trimmed) return text;
  let payload: unknown;
  try {
    payload = JSON.parse(trimmed);
  } catch {
    return text;
  }
  if (typeof payload !== "object" || payload === null || Array.isArray(payload)) return text;
  const root = payload as Record<string, unknown>;
  const env = root.env;
  if (typeof env !== "object" || env === null || Array.isArray(env)) return text;
  const envRecord = env as Record<string, unknown>;
  const hasApiKey = scalar(envRecord["ANTHROPIC_API_KEY"]) !== null;
  const authToken = scalar(envRecord["ANTHROPIC_AUTH_TOKEN"]);
  if (hasApiKey || authToken === null) {
    return text;
  }
  // 仅复制密钥值本身，不复制任何其他环境变量到 API_KEY 名下。
  const normalized: Record<string, unknown> = {
    ...root,
    env: { ...envRecord, ANTHROPIC_API_KEY: authToken },
  };
  return JSON.stringify(normalized, null, 2);
}
