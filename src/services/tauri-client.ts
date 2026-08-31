import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  ByokCredential,
  ChatCompletion,
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
) => invoke<WorkflowAgentCompletion>("run_workflow_agent", {
  request: { projectId, prompt },
  onEvent,
});

export const respondWorkflowApproval = (
  projectId: string,
  approvalId: string,
  approved: boolean,
  feedback?: string,
) => invoke<void>("respond_workflow_approval", {
  projectId,
  approvalId,
  approved,
  feedback,
});

export const cancelWorkflowAgent = (projectId: string) =>
  invoke<boolean>("cancel_workflow_agent", { projectId });

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
