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

export type RunEvent =
  | { type: "started"; runId: string; threadId: string; requestId: string }
  | { type: "status"; status: string; message: string }
  | { type: "text"; text: string; eventId?: string }
  | { type: "done"; runId: string; status: string; text: string; context: ServerRunContext };
