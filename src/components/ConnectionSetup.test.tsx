// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ConnectionSetup } from "./ConnectionSetup";
import { saveConnectionWithLogin } from "../services/tauri-client";

vi.mock("../services/tauri-client", () => ({
  activateWithCode: vi.fn(),
  saveConnectionWithLogin: vi.fn(),
  saveConnection: vi.fn(),
  pollDeviceLogin: vi.fn(),
  startDeviceLogin: vi.fn(),
  normalizeCommandError: (error: { message: string }) => new Error(error.message),
}));

describe("ConnectionSetup", () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("保留因临时服务故障而未保存的登录信息，便于直接重试", async () => {
    vi.mocked(saveConnectionWithLogin).mockRejectedValueOnce({
      code: "local_service_unavailable",
      message: "本机 Yuxi 服务未就绪",
      retryable: true,
    });
    render(
      <ConnectionSetup
        defaultGatewayUrl="http://127.0.0.1:9088"
        onConnected={vi.fn()}
      />,
    );

    const loginName = screen.getByPlaceholderText("管理员发放的登录 ID");
    const loginPassword = screen.getByPlaceholderText("管理员发放的初始密码");
    const apiKey = screen.getByPlaceholderText("yxkey_...");
    fireEvent.change(loginName, { target: { value: "tester01" } });
    fireEvent.change(loginPassword, { target: { value: "testpass12345" } });
    fireEvent.change(apiKey, { target: { value: "test-key-not-a-secret" } });
    fireEvent.click(screen.getByRole("button", { name: "测试并安全保存" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("本机 Yuxi 服务未就绪");
    await waitFor(() => {
      expect(loginName).toHaveValue("tester01");
      expect(apiKey).toHaveValue("test-key-not-a-secret");
    });
  });

  it("三要素校验成功后立即进入桌面端主界面", async () => {
    const settings = {
      gatewayUrl: "http://127.0.0.1:9088",
      agentSlug: "rice-endosperm-agent",
      hasApiKey: true,
      apiKeyHint: "yxkey_123456••••••••",
    };
    vi.mocked(saveConnectionWithLogin).mockResolvedValueOnce(settings);
    const onConnected = vi.fn();
    render(
      <ConnectionSetup
        defaultGatewayUrl="http://127.0.0.1:9088"
        onConnected={onConnected}
      />,
    );

    fireEvent.change(screen.getByPlaceholderText("管理员发放的登录 ID"), {
      target: { value: "tester01" },
    });
    fireEvent.change(screen.getByPlaceholderText("管理员发放的初始密码"), {
      target: { value: "testpass12345" },
    });
    fireEvent.change(screen.getByPlaceholderText("yxkey_..."), {
      target: { value: "yxkey_1234567890abcdefghijklmnopqrstuv" },
    });
    fireEvent.click(screen.getByRole("button", { name: "测试并安全保存" }));

    await waitFor(() => expect(onConnected).toHaveBeenCalledWith(settings));
  });
});
