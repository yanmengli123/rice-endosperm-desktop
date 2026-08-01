export type CommandError = {
  code: string;
  message: string;
  retryable: boolean;
  status?: number;
};

export type PublicSettings = {
  gatewayUrl: string;
  agentSlug: string;
  hasApiKey: boolean;
  apiKeyHint?: string;
};

export type ThreadSummary = {
  id: string;
  title: string;
  updatedAt: string;
  preview: string;
};

export type LocalMessage = {
  id: string;
  role: "user" | "assistant";
  content: string;
  createdAt: string;
};

export type SendMessageRequest = {
  threadId: string;
  question: string;
  requestId: string;
};

export type ChatCompletion = {
  runId: string;
  threadId: string;
  requestId: string;
  status: string;
  text: string;
};

export type RunEvent =
  | { type: "started"; runId: string; threadId: string; requestId: string }
  | { type: "status"; status: string; message: string }
  | { type: "text"; text: string; eventId?: string }
  | { type: "done"; runId: string; status: string; text: string };
