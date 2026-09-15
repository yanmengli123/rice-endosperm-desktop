import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  ByokCredential,
  ChatCompletion,
  DeviceCodeStart,
  DeviceLoginPoll,
  DeviceSessionView,
  LegacyClaimResult,
  LocalMessage,
  ModelOption,
  ModelConfigurationResult,
  PublicSettings,
  PendingRunSync,
  PendingChatAttachment,
  QuotaSummary,
  RunEvent,
  ServerRunContext,
  SendMessageRequest,
  ThreadSummary,
  TraceSnapshot,
  TraceEventPage,
  UsageSummary,
  WorkflowArtifact,
  WorkflowEngineStatus,
  WorkflowAgentCompletion,
  WorkflowAgentEvent,
  WorkflowAgentTurn,
  WorkflowModelSettings,
  WorkflowEvent,
  WorkflowProject,
  WorkflowRun,
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

/** P5 企业激活码开户：一次性激活码兑换设备会话对（不产生静态 Key）。 */
export const activateEnterpriseAccount = (
  activationCode: string,
  gatewayUrl: string,
  deviceName?: string,
) =>
  invoke<PublicSettings>("activate_enterprise_account", {
    activationCode,
    gatewayUrl,
    deviceName: deviceName ?? null,
  });

/** P2b 设备码第一步：创建待授权会话。 */
export const beginDeviceLogin = (gatewayUrl: string) =>
  invoke<DeviceCodeStart>("begin_device_login", { gatewayUrl });

/** P2b 设备码轮询：pending = 等待浏览器授权。 */
export const pollDeviceLogin = (deviceCode: string, gatewayUrl: string) =>
  invoke<DeviceLoginPoll>("poll_device_login", { deviceCode, gatewayUrl });

/** 打开设备码浏览器授权页（Rust 侧校验 URL 归属后经 opener 打开）。 */
export const openAuthorizationPage = (url: string) =>
  invoke<void>("open_authorization_page", { url });

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

export const getRunTrace = (runId: string) =>
  invoke<TraceSnapshot>("get_run_trace", { runId });

export const getRunTraceEvents = (runId: string, afterSequence: number) =>
  invoke<TraceEventPage>("get_run_trace_events", { runId, afterSequence });

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

// P5 自省与会话管理
export const getUserQuota = () => invoke<QuotaSummary>("get_user_quota");

export const getUserUsage = (days?: number) =>
  invoke<UsageSummary>("get_user_usage", { days: days ?? null });

export const listAuthSessions = () =>
  invoke<DeviceSessionView[]>("list_auth_sessions");

export const revokeAuthSession = (sessionId: string) =>
  invoke<void>("revoke_auth_session", { familyId: sessionId });

export const claimLegacyHistory = () =>
  invoke<LegacyClaimResult>("claim_legacy_history");

export const pickWorkflowDirectory = () =>
  invoke<string | null>("pick_workflow_directory");

export const createWorkflowProject = (root: string, name?: string) =>
  invoke<WorkflowProject>("create_workflow_project", { root, name });

export const listWorkflowProjects = () =>
  invoke<WorkflowProject[]>("list_workflow_projects");

export const deleteWorkflowProject = (projectId: string) =>
  invoke<void>("delete_workflow_project", { projectId });

export const listWorkflowRuns = (projectId: string) =>
  invoke<WorkflowRun[]>("list_workflow_runs", { projectId });

export const listWorkflowArtifacts = (projectId: string) =>
  invoke<WorkflowArtifact[]>("list_workflow_artifacts", { projectId });

export const listWorkflowAgentTurns = (projectId: string) =>
  invoke<WorkflowAgentTurn[]>("list_workflow_agent_turns", { projectId });

export const getWorkflowEngineStatus = () =>
  invoke<WorkflowEngineStatus>("get_workflow_engine_status");

export const runCountsPcaWorkflow = (
  projectId: string,
  inputRelativePath: string,
  onEvent: Channel<WorkflowEvent>,
) => invoke<WorkflowRun>("run_counts_pca_workflow", {
  request: { projectId, inputRelativePath },
  onEvent,
});

export const cancelWorkflowRun = (runId: string) =>
  invoke<boolean>("cancel_workflow_run", { runId });

export const openWorkflowArtifact = (artifactId: string) =>
  invoke<void>("open_workflow_artifact", { artifactId });

export const bridgeWorkflowArtifactToQa = (artifactId: string) =>
  invoke<PendingChatAttachment>("bridge_workflow_artifact_to_qa", { artifactId });

export const getWorkflowModelSettings = () =>
  invoke<WorkflowModelSettings | null>("get_workflow_model_settings");

export const saveWorkflowModelSettings = (
  provider: WorkflowModelSettings["provider"],
  baseUrl: string,
  model: string,
  apiKey: string,
) => invoke<WorkflowModelSettings>("save_workflow_model_settings", {
  settings: { provider, baseUrl, model, apiKey },
});

export const deleteWorkflowModelSettings = () =>
  invoke<void>("delete_workflow_model_settings");

export const runWorkflowAgent = (
  projectId: string,
  prompt: string,
  onEvent: Channel<WorkflowAgentEvent>,
  runId?: string,
) => invoke<WorkflowAgentCompletion>("run_workflow_agent", {
  request: { projectId, prompt, runId },
  onEvent,
});

export const respondWorkflowApproval = (
  runId: string,
  approvalId: string,
  approved: boolean,
  feedback?: string,
) => invoke<void>("respond_workflow_approval", {
  runId,
  approvalId,
  approved,
  feedback,
});

export const cancelWorkflowAgent = (runId: string) =>
  invoke<boolean>("cancel_workflow_agent", { runId });

export function normalizeCommandError(
  error: unknown,
): Error & { code?: string; retryable?: boolean; status?: number; action?: string; traceId?: string } {
  if (typeof error === "object" && error !== null && "message" in error) {
    const commandError = error as {
      message: unknown;
      code?: unknown;
      retryable?: unknown;
      status?: unknown;
      action?: unknown;
      trace_id?: unknown;
      traceId?: unknown;
    };
    const normalized = new Error(String(commandError.message)) as Error & {
      code?: string;
      retryable?: boolean;
      status?: number;
      action?: string;
      traceId?: string;
    };
    if (typeof commandError.code === "string") normalized.code = commandError.code;
    if (typeof commandError.retryable === "boolean") normalized.retryable = commandError.retryable;
    if (typeof commandError.status === "number") normalized.status = commandError.status;
    if (typeof commandError.action === "string") normalized.action = commandError.action;
    const trace = commandError.traceId ?? commandError.trace_id;
    if (typeof trace === "string") normalized.traceId = trace;
    return normalized;
  }
  return new Error(typeof error === "string" ? error : "发生未知错误");
}
