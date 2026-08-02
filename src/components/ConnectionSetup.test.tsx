// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ConnectionSetup } from "./ConnectionSetup";
import { saveConnection } from "../services/tauri-client";

vi.mock("../services/tauri-client", () => ({
  saveConnection: vi.fn(),
  normalizeCommandError: (error: { message: string }) => new Error(error.message),
}));

describe("ConnectionSetup", () => {
  afterEach(() => {
    vi.clearAllMocks();
  });

  it("保留因临时服务故障而未保存的 API Key，便于直接重试", async () => {
    vi.mocked(saveConnection).mockRejectedValueOnce({
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

    const apiKey = screen.getByPlaceholderText("yxkey_...");
    fireEvent.change(apiKey, { target: { value: "test-key-not-a-secret" } });
    fireEvent.click(screen.getByRole("button", { name: "测试并安全保存" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("本机 Yuxi 服务未就绪");
    await waitFor(() => expect(apiKey).toHaveValue("test-key-not-a-secret"));
  });
});
