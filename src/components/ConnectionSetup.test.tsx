// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ConnectionSetup } from "./ConnectionSetup";
import {
  activateEnterpriseAccount,
  beginDeviceLogin,
  openAuthorizationPage,
  pollDeviceLogin,
  saveConnectionWithLogin,
} from "../services/tauri-client";

vi.mock("../services/tauri-client", () => ({
  saveConnectionWithLogin: vi.fn(),
  activateEnterpriseAccount: vi.fn(),
  beginDeviceLogin: vi.fn(),
  pollDeviceLogin: vi.fn(),
  openAuthorizationPage: vi.fn(async () => undefined),
  normalizeCommandError: (error: { message: string }) => new Error(error.message),
}));

const connectedSettings = {
  gatewayUrl: "http://127.0.0.1:9088",
  agentSlug: "rice-endosperm-agent",
  hasApiKey: true,
  apiKeyHint: undefined,
};

function renderSetup(onConnected = vi.fn()) {
  render(
    <ConnectionSetup
      defaultGatewayUrl="http://127.0.0.1:9088"
      onConnected={onConnected}
    />,
  );
  return onConnected;
}

describe("ConnectionSetup", () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("默认展示企业激活码 Tab，成功兑换后进入主界面且清空激活码", async () => {
    vi.mocked(activateEnterpriseAccount).mockResolvedValueOnce(connectedSettings);
    const onConnected = renderSetup();

    // 默认 Tab 是激活码，不是兼容模式表单
    expect(screen.queryByPlaceholderText("管理员发放的登录 ID")).toBeNull();
    const codeInput = screen.getByPlaceholderText("yxact_...");
    fireEvent.change(codeInput, { target: { value: "yxact_0123456789abcdef" } });
    fireEvent.click(screen.getByRole("button", { name: "激活并安全保存" }));

    await waitFor(() => expect(onConnected).toHaveBeenCalledWith(connectedSettings));
    expect(activateEnterpriseAccount).toHaveBeenCalledWith(
      "yxact_0123456789abcdef",
      "http://127.0.0.1:9088",
      undefined,
    );
    expect((codeInput as HTMLInputElement).value).toBe("");
  });

  it("激活码 410 过期错误按服务端文案分流展示", async () => {
    vi.mocked(activateEnterpriseAccount).mockRejectedValueOnce({
      code: "expired",
      message: "激活码已过期",
      retryable: false,
      status: 410,
    });
    renderSetup();

    fireEvent.change(screen.getByPlaceholderText("yxact_..."), {
      target: { value: "yxact_expired00000000" },
    });
    fireEvent.click(screen.getByRole("button", { name: "激活并安全保存" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("激活码已过期");
  });

  it("兼容模式（三要素）校验失败时保留登录信息便于重试", async () => {
    vi.mocked(saveConnectionWithLogin).mockRejectedValueOnce({
      code: "local_service_unavailable",
      message: "本机 Yuxi 服务未就绪",
      retryable: true,
    });
    renderSetup();

    fireEvent.click(screen.getByRole("tab", { name: /账号密码/ }));
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

  it("兼容模式三要素校验成功后立即进入桌面端主界面", async () => {
    vi.mocked(saveConnectionWithLogin).mockResolvedValueOnce({
      ...connectedSettings,
      apiKeyHint: "yxkey_123456••••••••",
    });
    const onConnected = renderSetup();

    fireEvent.click(screen.getByRole("tab", { name: /账号密码/ }));
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

    await waitFor(() => expect(onConnected).toHaveBeenCalled());
  });

  it("设备码授权：展示确认码并轮询到批准后进入主界面", async () => {
    vi.mocked(beginDeviceLogin).mockResolvedValueOnce({
      deviceCode: "dc-1",
      userCode: "ABCD-1234",
      verificationUri: "https://web.example.cn/auth/cli/authorize",
      verificationUriComplete: "https://web.example.cn/auth/cli/authorize?user_code=ABCD-1234",
      expiresIn: 600,
      interval: 2,
    });
    vi.mocked(pollDeviceLogin)
      .mockResolvedValueOnce({ status: "pending" })
      .mockResolvedValueOnce({ status: "pending" })
      .mockResolvedValueOnce({ status: "done", settings: connectedSettings });
    const onConnected = renderSetup();

    fireEvent.click(screen.getByRole("tab", { name: /浏览器授权/ }));
    fireEvent.click(screen.getByRole("button", { name: "开始浏览器授权" }));

    // 确认码展示给用户核对（防钓鱼：不能只依赖预填 URL）
    expect(await screen.findByText("ABCD-1234")).toBeVisible();
    expect(openAuthorizationPage).toHaveBeenCalledWith(
      "https://web.example.cn/auth/cli/authorize?user_code=ABCD-1234",
    );
    await waitFor(() => expect(onConnected).toHaveBeenCalledWith(connectedSettings), {
      timeout: 8000,
    });
  });

  it("设备码授权过期（410）停止轮询并展示服务端文案", async () => {
    vi.mocked(beginDeviceLogin).mockResolvedValueOnce({
      deviceCode: "dc-2",
      userCode: "WXYZ-4321",
      verificationUri: "https://web.example.cn/auth/cli/authorize",
      verificationUriComplete: "https://web.example.cn/auth/cli/authorize?user_code=WXYZ-4321",
      expiresIn: 600,
      interval: 2,
    });
    vi.mocked(pollDeviceLogin).mockRejectedValue({
      code: "expired_token",
      message: "授权会话已过期",
      retryable: false,
      status: 410,
    });
    renderSetup();

    fireEvent.click(screen.getByRole("tab", { name: /浏览器授权/ }));
    fireEvent.click(screen.getByRole("button", { name: "开始浏览器授权" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("授权会话已过期");
    expect(screen.queryByText("WXYZ-4321")).toBeNull();
  });
});
