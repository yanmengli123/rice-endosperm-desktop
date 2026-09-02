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

export type WorkflowProject = {
  id: string;
  name: string;
  root: string;
  createdAt: string;
  updatedAt: string;
};

export type WorkflowRun = {
  id: string;
  projectId: string;
  workflowKind: string;
  status: "queued" | "running" | "completed" | "failed" | "cancelled" | "interrupted";
  inputPath?: string;
  manifestPath?: string;
  summaryJson: string;
  error?: string;
  createdAt: string;
  startedAt?: string;
  finishedAt?: string;
};

export type WorkflowArtifact = {
  id: string;
  runId: string;
  projectId: string;
  name: string;
  relativePath: string;
  mediaType: string;
  sizeBytes: number;
  sha256: string;
  createdAt: string;
};

export type WorkflowEngineStatus = {
  protocol: string;
  available: boolean;
  runningProjects: number;
  workerPath?: string;
  workerVersion?: string;
  message: string;
};

export type WorkflowModelSettings = {
  provider: "openai" | "openai_responses" | "anthropic";
  baseUrl: string;
  model: string;
  hasApiKey: boolean;
  apiKeyHint?: string;
};

export type WorkflowAgentCompletion = {
  turnId: string;
  text: string;
  sessionId?: string;
  inputTokens: number;
  outputTokens: number;
  reasoningTokens: number;
  changedPaths: string[];
};

export type WorkflowAgentTurn = {
  id: string;
  runId: string;
  projectId: string;
  engineTurnId?: string;
  engineSessionId?: string;
  provider: string;
  model: string;
  prompt: string;
  response: string;
  status: "running" | "completed" | "failed" | "cancelled" | "interrupted";
  error?: string;
  inputTokens: number;
  outputTokens: number;
  reasoningTokens: number;
  createdAt: string;
  finishedAt?: string;
};

export type WorkflowAgentEvent =
  | { type: "engine_ready"; protocol: string; model: string; root: string }
  | { type: "turn_started"; turn_id: string }
  | { type: "progress"; phase: string; message: string; elapsed_ms: number }
  | { type: "text_delta"; delta: string }
  | { type: "reasoning_active" }
  | { type: "tool_started"; call_id?: string; name: string; preview: string }
  | { type: "tool_finished"; call_id?: string; name: string; ok: boolean; content: string; duration_ms: number }
  | { type: "approval_required"; approval_id: string; message: string }
  | { type: "file_changed"; path: string }
  | { type: "usage"; input_tokens: number; output_tokens: number; reasoning_tokens: number }
  | { type: "turn_completed"; ok: boolean; error?: string }
  | { type: "engine_error"; message: string };

export type WorkflowEvent =
  | { type: "run_started"; run_id: string; message: string }
  | { type: "progress"; run_id: string; percent: number; message: string }
  | { type: "artifact_created"; run_id: string; artifact: WorkflowArtifact }
  | { type: "run_completed"; run: WorkflowRun }
  | { type: "run_failed"; run_id: string; message: string }
  | { type: "run_cancelled"; run_id: string };

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
  attachments: LocalMessageAttachment[];
};

export type LocalMessageAttachment = {
  id: string;
  name: string;
  contentType?: string;
  fileSize: number;
};

export type SendMessageRequest = {
  threadId: string;
  question: string;
  requestId: string;
  attachments: PendingChatAttachment[];
};

export type PendingChatAttachment = {
  tmpFileId: string;
  fileName: string;
  fileType?: string;
  fileSize: number;
  bucketName: string;
  objectName: string;
  parseSupported: boolean;
  parseMethods: string[];
  parsedObjectName?: string;
  parseMethod?: string;
  truncated: boolean;
};

export type ChatCompletion = {
  runId: string;
  threadId: string;
  requestId: string;
  status: string;
  text: string;
  context: ServerRunContext;
};

export type KnowledgeScopeMember = {
  kbId?: string;
  kbName?: string;
  kbType?: string;
  priority?: number;
  documentEnabled: boolean;
  graphEnabled: boolean;
  structuredEnabled: boolean;
  includedVia?: string;
};

export type KnowledgeScopeSummary = {
  scopeId?: string;
  scopeVersion?: number;
  scopeMode?: string;
  knowledgeStrategy?: string;
  retrievalMode?: string;
  allowWeb: boolean;
  kbCount: number;
  members: KnowledgeScopeMember[];
};

export type KnowledgeRetrievalSummary = {
  retrievalId?: string;
  status?: string;
  intent?: string;
  queryMode?: string;
  plannerVersion?: string;
  entityResolverVersion?: string;
  retrievalOrchestratorVersion?: string;
  claimValidatorVersion?: string;
  contractSchemaVersion?: string;
  sourceStatus: unknown[];
  returnedRelationCount?: number;
  returnedClaimCount?: number;
  returnedEvidenceCount?: number;
  warnings: unknown[];
  errorCode?: string;
  finishedAt?: string;
};

export type ServerRunContext = {
  protocolVersion?: string;
  modelSpec?: string;
  knowledgeScope: KnowledgeScopeSummary;
  knowledgeRetrievals: KnowledgeRetrievalSummary[];
};

export type PendingRunSync = {
  recovered: number;
  pending: number;
  failed: number;
  lastError?: string;
};

export type ByokCredential = {
  credentialId: number;
  providerId: string;
  label: string;
  maskedHint: string;
  status: string;
  protocol?: string;
  baseUrl?: string;
  modelId?: string;
  modelSpec?: string;
};

export type ModelConfigurationResult = {
  credentialId: number;
  modelSpec: string;
  ignoredFields: string[];
};

export type ModelOption = {
  spec: string;
  label: string;
};

export type RunEvent =
  | { type: "started"; runId: string; threadId: string; requestId: string }
  | { type: "status"; status: string; message: string }
  | { type: "text"; text: string; eventId?: string }
  | { type: "done"; runId: string; status: string; text: string; context: ServerRunContext };
