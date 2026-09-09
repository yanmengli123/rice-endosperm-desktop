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
  | { type: "trace"; runId: string; trace: TraceEvent }
  | { type: "done"; runId: string; status: string; text: string; context: ServerRunContext };

/** yuxi.run-trace.v1 wire 事件（Rust 侧 Value 原样透传，保持服务端 snake_case）。 */
export type TraceEvent = {
  schema_version?: string;
  event_id?: string;
  trace_id?: string;
  sequence: number;
  run_id?: string;
  thread_id?: string;
  category: string;
  operation: string;
  event_type: string;
  span_id?: string;
  parent_span_id?: string;
  occurred_at?: string;
  duration_ms?: number | null;
  title?: string | null;
  summary?: string | null;
  message_key?: string | null;
  display_args?: Record<string, unknown>;
  attributes?: Record<string, unknown>;
  resource_refs?: { type: string; id: string }[];
  visibility?: string;
};

export type TraceSpan = {
  span_id: string;
  parent_span_id?: string | null;
  category: string;
  operation?: string;
  title?: string | null;
  summary?: string | null;
  message_key?: string | null;
  display_args?: Record<string, unknown>;
  visibility?: string;
  status: string;
  started_at?: string | null;
  finished_at?: string | null;
  duration_ms?: number | null;
  error_type?: string | null;
  retry_count?: number;
  attributes?: Record<string, unknown>;
};

export type TraceSummary = {
  run_id?: string;
  status?: string | null;
  agent_slug?: string | null;
  duration_ms?: number | null;
  ttft_ms?: number | null;
  total_tokens?: number | null;
  model_calls?: number;
  tool_calls?: number;
  mcp_calls?: number;
  knowledge_calls?: number;
  subagent_calls?: number;
  skill_count?: number;
  retry_count?: number;
  error_count?: number;
};

export type TraceSnapshot = {
  run_id: string;
  summary: TraceSummary | null;
  spans: TraceSpan[];
  snapshot_sequence: number;
  projection_sequence: number;
};

export type TraceEventPage = {
  run_id: string;
  events: TraceEvent[];
  next_after_sequence: number;
  scanned_through_sequence: number;
  has_more: boolean;
};
