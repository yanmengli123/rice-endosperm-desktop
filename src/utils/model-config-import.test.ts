import { describe, expect, it } from "vitest";
import { normalizeConfigurationForServer, parseModelConfigurationJson } from "./model-config-import";

const MINIMAX_CLAUDE_CODE_JSON = `{
  "env": {
    "ANTHROPIC_AUTH_TOKEN": "sk-cp-test-token-0000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    "ANTHROPIC_BASE_URL": "https://api.minimaxi.com/anthropic",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "MiniMax-M3",
    "ANTHROPIC_DEFAULT_OPUS_MODEL": "MiniMax-M3",
    "ANTHROPIC_DEFAULT_SONNET_MODEL": "MiniMax-M3",
    "ANTHROPIC_MODEL": "MiniMax-M3",
    "ANTHROPIC_REASONING_MODEL": "MiniMax-M3",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME": "MiniMax-M3",
    "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME": "MiniMax-M3",
    "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME": "MiniMax-M3",
    "API_TIMEOUT_MS": "300000",
    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": 1
  },
  "includeCoAuthoredBy": false
}`;

describe("parseModelConfigurationJson", () => {
  it("parses the MiniMax Claude Code export with AUTH_TOKEN alias", () => {
    const preview = parseModelConfigurationJson(MINIMAX_CLAUDE_CODE_JSON);
    expect(preview.ok).toBe(true);
    expect(preview.error).toBeUndefined();
    expect(preview.protocol).toBe("anthropic");
    expect(preview.baseUrl).toBe("https://api.minimaxi.com/anthropic");
    expect(preview.model).toBe("MiniMax-M3");
    expect(preview.maskedApiKey).toBe("sk-cp-****0000");
    expect(preview.resolvedSources).toEqual({
      apiKey: "ANTHROPIC_AUTH_TOKEN",
      baseUrl: "ANTHROPIC_BASE_URL",
      model: "ANTHROPIC_MODEL",
    });
    expect(preview.ignoredFields).toEqual([
      "API_TIMEOUT_MS",
      "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC",
      "includeCoAuthoredBy",
    ]);
  });

  it("falls back through the documented model alias chain", () => {
    const preview = parseModelConfigurationJson(
      JSON.stringify({
        env: {
          ANTHROPIC_AUTH_TOKEN: "sk-test-key-000000",
          ANTHROPIC_BASE_URL: "https://api.example.org",
          ANTHROPIC_DEFAULT_HAIKU_MODEL: "haiku-model",
          ANTHROPIC_DEFAULT_SONNET_MODEL: "sonnet-model",
        },
      }),
    );
    expect(preview.ok).toBe(true);
    expect(preview.model).toBe("sonnet-model");
    expect(preview.resolvedSources.model).toBe("ANTHROPIC_DEFAULT_SONNET_MODEL");
  });

  it("explains missing fields with the alias chain", () => {
    const preview = parseModelConfigurationJson(
      JSON.stringify({ env: { ANTHROPIC_BASE_URL: "https://api.example.org" } }),
    );
    expect(preview.ok).toBe(false);
    expect(preview.error).toContain("ANTHROPIC_API_KEY");
    expect(preview.error).toContain("ANTHROPIC_AUTH_TOKEN");
  });

  it("rejects non-https base urls before hitting the server", () => {
    const preview = parseModelConfigurationJson(
      JSON.stringify({
        env: {
          ANTHROPIC_API_KEY: "sk-test",
          ANTHROPIC_BASE_URL: "http://api.example.org",
          ANTHROPIC_MODEL: "model",
        },
      }),
    );
    expect(preview.ok).toBe(false);
    expect(preview.error).toContain("HTTPS");
  });

  it("reports json syntax errors without echoing the payload", () => {
    const preview = parseModelConfigurationJson('{"env": {');
    expect(preview.ok).toBe(false);
    expect(preview.error).toContain("JSON 格式错误");
  });

  it("rejects non-object roots and missing env", () => {
    expect(parseModelConfigurationJson("[]").error).toContain("根节点");
    expect(parseModelConfigurationJson("{}").error).toContain("env");
  });

  it("keeps empty input at a friendly error", () => {
    expect(parseModelConfigurationJson("   ").error).toContain("请先粘贴");
  });
});

describe("normalizeConfigurationForServer", () => {
  it("adds an ANTHROPIC_API_KEY alias when only AUTH_TOKEN is provided", () => {
    const normalized = normalizeConfigurationForServer(
      JSON.stringify({
        env: {
          ANTHROPIC_AUTH_TOKEN: "sk-test-token",
          ANTHROPIC_BASE_URL: "https://api.example.org",
          ANTHROPIC_MODEL: "model-x",
        },
        includeCoAuthoredBy: false,
      }),
    );
    const payload = JSON.parse(normalized) as {
      env: Record<string, unknown>;
      includeCoAuthoredBy: boolean;
    };
    // 兼容旧版服务端：只认 ANTHROPIC_API_KEY
    expect(payload.env.ANTHROPIC_API_KEY).toBe("sk-test-token");
    // 其余字段原样保留，不执行额外复制
    expect(payload.env.ANTHROPIC_AUTH_TOKEN).toBe("sk-test-token");
    expect(payload.env.ANTHROPIC_BASE_URL).toBe("https://api.example.org");
    expect(payload.env.ANTHROPIC_MODEL).toBe("model-x");
    expect(payload.includeCoAuthoredBy).toBe(false);
    expect(Object.keys(payload.env).length).toBe(4);
  });

  it("keeps input unchanged when ANTHROPIC_API_KEY is already present", () => {
    const input = JSON.stringify({
      env: {
        ANTHROPIC_API_KEY: "sk-primary",
        ANTHROPIC_AUTH_TOKEN: "sk-secondary",
        ANTHROPIC_MODEL: "m",
      },
    });
    expect(normalizeConfigurationForServer(input)).toBe(input);
  });

  it("keeps input unchanged for malformed or non-json text", () => {
    expect(normalizeConfigurationForServer("not json")).toBe("not json");
    expect(normalizeConfigurationForServer('{"env": {')).toBe('{"env": {');
    expect(normalizeConfigurationForServer("[]")).toBe("[]");
    expect(normalizeConfigurationForServer("{}")).toBe("{}");
  });
});
