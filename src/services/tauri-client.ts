import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  ActivationOutcome,
  ByokCredential,
  ChatCompletion,
  DeviceLoginResult,
  DeviceLoginStart,
  LocalMessage,
  ModelOption,
  ModelConfigurationResult,
  PublicSettings,
  PendingRunSync,
  PendingChatAttachment,
  RunEvent,
  ServerRunContext,
  SendMessageRequest,
  ThreadSummary,
} from "../types";

export const getPublicSettings = () =>
  invoke<PublicSettings>("get_public_settings");

export const saveConnectionWithLogin = (
  apiKey: string,
  gatewayUrl: string,
  username: string,
  password: string,
) =>
  invoke<PublicSettings>("save_connection_with_login", {
    apiKey,
    gatewayUrl,
    username,
    password,
  });

export const saveConnection = (apiKey: string, gatewayUrl: string) =>
  invoke<PublicSettings>("save_connection", { apiKey, gatewayUrl });

export const testConnection = () => invoke<void>("test_connection");

export const deleteApiKey = () => invoke<void>("delete_api_key");

export const createThread = () => invoke<ThreadSummary>("create_thread");

export const listThreads = () => invoke<ThreadSummary[]>("list_threads");

export const loadMessages = (threadId: string) =>
  invoke<LocalMessage[]>("load_messages", { threadId });

export const getThreadRunContext = (threadId: string) =>
  invoke<ServerRunContext | null>("get_thread_run_context", { threadId });

export const syncPendingRuns = () =>
  invoke<PendingRunSync>("sync_pending_runs");

export const renameThread = (threadId: string, title: string) =>
  invoke<void>("rename_thread", { threadId, title });

export const deleteThread = (threadId: string) =>
  invoke<void>("delete_thread", { threadId });

export const sendMessage = (
  request: SendMessageRequest,
  onEvent: Channel<RunEvent>,
) => invoke<ChatCompletion>("send_message", { request, onEvent });

export const uploadChatAttachment = (
  fileName: string,
  contentType: string,
  dataBase64: string,
) => invoke<PendingChatAttachment>("upload_chat_attachment", { fileName, contentType, dataBase64 });

export const parseChatAttachment = (
  attachment: PendingChatAttachment,
  parseMethod: string,
) => invoke<PendingChatAttachment>("parse_chat_attachment", { attachment, parseMethod });

export const cancelRun = (requestId: string, runId?: string) =>
  invoke<void>("cancel_run", { requestId, runId });

export const startDeviceLogin = (gatewayUrl: string, keyName?: string) =>
  invoke<DeviceLoginStart>("start_device_login", { gatewayUrl, keyName });

export const pollDeviceLogin = (gatewayUrl: string, deviceCode: string) =>
  invoke<DeviceLoginResult>("poll_device_login", { gatewayUrl, deviceCode });

export const activateWithCode = (gatewayUrl: string, activationCode: string, deviceName?: string) =>
  invoke<ActivationOutcome>("activate_with_code", {
    gatewayUrl,
    activationCode,
    deviceName: deviceName ?? null,
  });

export const listChatModels = () => invoke<ModelOption[]>("list_chat_models");

export const listByokCredentials = () => invoke<ByokCredential[]>("list_byok_credentials");

export const saveByokCredential = (providerId: string, apiKey: string) =>
  invoke<void>("save_byok_credential", { providerId, apiKey });

export const saveCustomModelCredential = (
  protocol: "openai" | "anthropic",
  baseUrl: string,
  apiKey: string,
  model: string,
) => invoke<ModelConfigurationResult>("save_custom_model_credential", {
  protocol,
  baseUrl,
  apiKey,
  model,
});

export const importModelConfiguration = (configuration: string) =>
  invoke<ModelConfigurationResult>("import_model_configuration", { configuration });

export const removeByokCredential = (credentialId: number) =>
  invoke<void>("remove_byok_credential", { credentialId });

export const getChatModelPreference = () =>
  invoke<string | null>("get_chat_model_preference");

export const setChatModelPreference = (modelSpec?: string) =>
  invoke<void>("set_chat_model_preference", { modelSpec });

// P2b 多账号：目录、切换与移除
export interface AccountSummary {
  accountScope: string;
  displayName: string;
  gatewayUrl: string;
  isActive: boolean;
}

export const listAccounts = () => invoke<AccountSummary[]>("list_accounts");

export const switchAccount = (accountScope: string) =>
  invoke<void>("switch_account", { accountScope });

export const removeAccount = (accountScope: string) =>
  invoke<void>("remove_account", { accountScope });

export function normalizeCommandError(
  error: unknown,
): Error & { code?: string; retryable?: boolean; status?: number } {
  if (typeof error === "object" && error !== null && "message" in error) {
    const commandError = error as {
      message: unknown;
      code?: unknown;
      retryable?: unknown;
      status?: unknown;
    };
    const normalized = new Error(String(commandError.message)) as Error & {
      code?: string;
      retryable?: boolean;
      status?: number;
    };
    if (typeof commandError.code === "string") normalized.code = commandError.code;
    if (typeof commandError.retryable === "boolean") normalized.retryable = commandError.retryable;
    if (typeof commandError.status === "number") normalized.status = commandError.status;
    return normalized;
  }
  return new Error(typeof error === "string" ? error : "发生未知错误");
}
