// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SettingsDialog } from "./SettingsDialog";
import {
  getChatModelPreference,
  listChatModels,
  setChatModelPreference,
} from "../services/tauri-client";

vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn() }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("../services/tauri-client", () => ({
  deleteApiKey: vi.fn(),
  getChatModelPreference: vi.fn(),
  listChatModels: vi.fn(),
  normalizeCommandError: (error: { message?: string }) => new Error(error.message ?? "request failed"),
  setChatModelPreference: vi.fn(),
  testConnection: vi.fn(),
}));

const settings = {
  gatewayUrl: "https://rice.example.cn",
  agentSlug: "chatbot",
  apiKeyHint: "yxkey_123456••••••••",
  hasApiKey: true,
};

describe("SettingsDialog model preference", () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("does not display a false default when the server preference fails to load", async () => {
    vi.mocked(listChatModels).mockResolvedValue([]);
    vi.mocked(getChatModelPreference).mockRejectedValue(new Error("service unavailable"));

    render(
      <SettingsDialog
        settings={settings}
        onClose={vi.fn()}
        onCredentialDeleted={vi.fn()}
      />,
    );

    const select = await screen.findByLabelText("选择默认聊天模型");
    await waitFor(() => expect(select).toBeDisabled());
    expect(select).toHaveDisplayValue("模型偏好暂不可用");
    expect(screen.getByText(/模型偏好加载失败/)).toBeInTheDocument();
  });

  it("rolls back the selected model when the server rejects the update", async () => {
    vi.mocked(listChatModels).mockResolvedValue([
      { spec: "provider:model-a", label: "Model A" },
      { spec: "provider:model-b", label: "Model B" },
    ]);
    vi.mocked(getChatModelPreference).mockResolvedValue("provider:model-a");
    vi.mocked(setChatModelPreference).mockRejectedValue(new Error("save failed"));

    render(
      <SettingsDialog
        settings={settings}
        onClose={vi.fn()}
        onCredentialDeleted={vi.fn()}
      />,
    );

    const select = await screen.findByLabelText("选择默认聊天模型");
    await waitFor(() => expect(select).toHaveValue("provider:model-a"));
    fireEvent.change(select, { target: { value: "provider:model-b" } });

    await waitFor(() => expect(setChatModelPreference).toHaveBeenCalledWith("provider:model-b"));
    await waitFor(() => expect(select).toHaveValue("provider:model-a"));
    expect(screen.getByText("save failed")).toBeInTheDocument();
  });
});
