import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  ChatCompletion,
  LocalMessage,
  PublicSettings,
  PendingRunSync,
  RunEvent,
  ServerRunContext,
  SendMessageRequest,
  ThreadSummary,
} from "../types";

export const getPublicSettings = () =>
  invoke<PublicSettings>("get_public_settings");

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

export const cancelRun = (requestId: string, runId?: string) =>
  invoke<void>("cancel_run", { requestId, runId });

export function normalizeCommandError(error: unknown): Error & { code?: string } {
  if (typeof error === "object" && error !== null && "message" in error) {
    const commandError = error as { message: unknown; code?: unknown };
    const normalized = new Error(String(commandError.message)) as Error & {
      code?: string;
    };
    if (typeof commandError.code === "string") normalized.code = commandError.code;
    return normalized;
  }
  return new Error(typeof error === "string" ? error : "发生未知错误");
}
