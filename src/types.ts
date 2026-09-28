export type CommandError = {
  code: string;
  message: string;
  retryable: boolean;
  status?: number;
  action?: string;
  traceId?: string;
};

export type PublicSettings = {
  gatewayUrl: string;
  agentSlug: string;
  hasApiKey: boolean;
  apiKeyHint?: string;
};

/** 设备码创建结果（begin_device_login）。 */
export type DeviceCodeStart = {
  deviceCode: string;
  userCode: string;
  verificationUri: string;
  verificationUriComplete: string;
  expiresIn: number;
  interval: number;
};

/** 设备码轮询结果：pending = 等待浏览器授权。 */
export type DeviceLoginPoll = {
  status: "pending" | "done";
  settings?: PublicSettings;
};

/** 用户权益（GET /api/user/quota）。 */
export type QuotaSummary = {
  dailyRunLimit?: number;
  monthlyTokenLimit?: number;
  modelAccessPolicy: string;
  byokPlatformTokenExempt: boolean;
  hasActiveByok: boolean;
};

/** 单日用量。 */
export type UsageDay = {
  date: string;
  runCount: number;
  tokens: number;
};

/** 用户用量（GET /api/user/usage）。 */
export type UsageSummary = {
  daily: UsageDay[];
  monthlyTokens: number;
  monthlyPlatformTokens: number;
  monthlyByokTokens: number;
};

/** 设备会话摘要（账号与安全）。 */
export type DeviceSessionView = {
  sessionId: string;
  createdAt?: string;
  lastRefreshedAt?: string;
};

/** legacy 历史认领结果。 */
export type LegacyClaimResult = {
  claimedThreads: number;
  claimedMessages: number;
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
  /** 证据平面元数据（citation 芯片/图卡/候选文献/运行提示）：仅在 SSE 流上
   * 实际观测到时非空，重开历史会话时从本地真源恢复。 */
  evidence?: MessageEvidence;
};

/** citation_ready v2 的九键引用：基线七键 + 取图三段式硬前置（kbId/revisionId）。
 * 所有键均可缺席——渲染层按实值展示，缺席不猜测补全。 */
export type CitationRef = {
  status?: string;
  evidenceId?: string;
  fileId?: string;
  filename?: string;
  zone?: string;
  page?: number;
  anchorId?: string;
  kbId?: string;
  revisionId?: string;
};

/** 图卡选择元数据（PublishableFigure.selection，服务端确定性投影）。 */
export type FigureSelection = {
  assetCount?: number;
  rule?: string;
};

/** 服务端 PublishableFigure 完整投影。字段全部可缺席：硬前置四键
 * （kbId/fileId/revisionId/assetName）齐备才可取图，缺键 fail-closed 不渲染图卡。 */
export type FigureCardAsset = {
  bindingId?: string;
  kbId?: string;
  fileId?: string;
  revisionId?: string;
  assetName?: string;
  assetSha256?: string;
  mediaType?: string;
  page?: number;
  panelMatch?: string;
  evidenceId?: string;
  figureLabel?: string;
  caption?: string;
  width?: number;
  height?: number;
  role?: string;
  groupIndex?: number;
  panelLabel?: string;
  assetPage?: number;
  selection?: FigureSelection;
};

/** locator_candidates 候选文献（跨文献歧义清单，只含文档身份三键）。 */
export type LocatorCandidate = {
  fileId: string;
  kbId: string;
  filename: string;
};

/** 图表锚点绑定（citation_ready v3 figure_refs，ADR-0008）：正文〔图表F#〕
 * 签发芯片与消息级图卡/证据原文的桥接。figureIndex 指向同载荷 figures[]
 * 下标（null = 无卡片，Table/suppressed 经 evidenceId 跳原文）。 */
export type FigureRef = {
  ref: string;
  kind?: string;
  label?: string;
  source?: string;
  citationRef?: string;
  evidenceId?: string;
  anchorId?: string;
  kbId?: string;
  fileId?: string;
  revisionId?: string;
  page?: number;
  figureIndex?: number | null;
  tableIndex?: number | null;
  suppressedReason?: string | null;
};

/** 表格单元格（ADR-0008 P2 受控解析）：纯文本 + 数值跨度，无任何标记语言。 */
export type TableCell = {
  text?: string;
  rowspan?: number;
  colspan?: number;
  header?: boolean;
};

/** 服务端 PublishableTable 投影（citation_ready.tables[]）。rows 为行列 JSON
 * （永不携带原始 HTML），渲染层只做文本插值（受控渲染）。 */
export type PublishableTable = {
  projectionVersion?: string;
  tableId?: string;
  kbId?: string;
  fileId?: string;
  revisionId?: string;
  anchorId?: string;
  evidenceId?: string;
  label?: string;
  caption?: string;
  page?: number;
  headerRows?: number;
  rows?: TableCell[][];
  rowCount?: number;
  colCount?: number;
  truncated?: boolean;
  /** 规模护栏截断（行/列/单元格上限，后端 limited）：与跨页延续块区分，文案分流 */
  limited?: boolean;
  /** 服务端确定性投影审计元数据；客户端只持久化/透传，不参与权威裁决。 */
  selection?: Record<string, unknown>;
};

export type GraphSnapshotNode = {
  entity_id: string;
  kb_id: string;
  canonical_identity?: string;
  name: string;
  label: string;
  is_seed?: boolean;
};

export type GraphSnapshotEdge = {
  triple_id: string;
  triple_ids?: string[];
  reviewed_triple_ids?: string[];
  candidate_triple_ids?: string[];
  kb_id: string;
  source_entity_id: string;
  target_entity_id: string;
  predicate: string;
  relation_group?: string;
  predicates?: string[];
  parallel_count?: number;
  reviewed_parallel_count?: number;
  candidate_parallel_count?: number;
  review_status?: string;
  review_version?: number;
  support_count?: number;
  literature_count?: number;
  conflict_status?: string;
  risk_score?: number | null;
};

export type GraphSnapshot = {
  schema: "graph_snapshot_v1";
  retrieval_id?: string;
  outcome: "HIT" | "MISS" | "PENDING_REVIEW" | "AMBIGUOUS" | "UNAVAILABLE";
  scope_version?: number | string;
  review_policy?: "approved_only" | "approved_plus_candidate";
  seed_entity_ids?: string[];
  seed_names?: string[];
  seed_display_name?: string;
  total_raw_edge_count?: number;
  aggregation?: {
    strategy?: string;
    group_count?: number;
    seed_collapsed?: boolean;
  };
  suppressed?: {
    review_policy?: number;
    rejected?: number;
    edge_budget?: number;
    node_budget?: number;
  };
  projection_hash?: string;
  truncated?: boolean;
  conflict_present?: boolean;
  nodes: GraphSnapshotNode[];
  edges: GraphSnapshotEdge[];
};

/** 一次 run 观测到的证据平面快照（权威投影原样存档，恢复时不做二次推断）。 */
export type MessageEvidence = {
  citations: CitationRef[];
  figures: FigureCardAsset[];
  figureRefs?: FigureRef[];
  tables?: PublishableTable[];
  locatorCandidates: LocatorCandidate[];
  graphSnapshots?: GraphSnapshot[];
  warnings: string[];
};

/** GET /api/agent/protocol 能力快照（渲染分支的能力位真源）。 */
export type ProtocolInfo = {
  protocolVersion: string;
  minSupportedProtocolVersion?: string;
  capabilities: string[];
};

/** 图卡资产字节（base64 通道，前端拼 data URL；不落盘）。 */
export type FigureAssetPayload = {
  mediaType: string;
  dataBase64: string;
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
  /** 中断恢复（人工审批续跑）：被恢复的父 run ID。 */
  resumeRunId?: string;
  /** 澄清问题续跑（ask_user_question_required）：AskUserQuestion 事件原样
   * 回传的问题定义与用户作答；Rust 侧组装 {questionId: 答案} resume 载荷。 */
  resumeQuestions?: ClarificationQuestion[];
  resumeAnswers?: QuestionAnswer[];
};

/** 澄清问题选项（wire {label, value}，服务端归一化）。 */
export type QuestionOption = {
  label: string;
  value: string;
};

/** 服务端澄清问题（Rust 侧 camelCase IPC 形态）。 */
export type ClarificationQuestion = {
  questionId: string;
  question: string;
  options: QuestionOption[];
  multiSelect: boolean;
  allowOther: boolean;
};

/** 用户对单个澄清问题的作答（selected 含 "其他" 保留值时携带 otherText）。 */
export type QuestionAnswer = {
  questionId: string;
  selected: string[];
  otherText?: string;
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
  | { type: "ask_user_question"; runId: string; questions: ClarificationQuestion[] }
  | {
      type: "citations_ready";
      runId: string;
      citation: CitationRef;
      figures: FigureCardAsset[];
      figureRefs: FigureRef[];
      tables: PublishableTable[];
    }
  | { type: "locator_candidates"; runId: string; candidates: LocatorCandidate[] }
  | { type: "graph_snapshot_ready"; runId: string; snapshot: GraphSnapshot }
  | { type: "warning"; message: string }
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
