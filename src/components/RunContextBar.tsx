import { BookOpenCheck, BrainCircuit, Database, ShieldCheck, TriangleAlert } from "lucide-react";
import type { ServerRunContext } from "../types";

type Props = {
  context?: ServerRunContext | null;
};

function displayModel(modelSpec?: string) {
  const parts = modelSpec?.split(":") || [];
  return parts[parts.length - 1] || "由服务端选择";
}

export function RunContextBar({ context }: Props) {
  if (!context) return null;
  const scope = context.knowledgeScope;
  const latest = context.knowledgeRetrievals[context.knowledgeRetrievals.length - 1];
  const knowledgeNames = scope.members
    .map((member) => member.kbName || member.kbId)
    .filter(Boolean)
    .join("、");
  const hasRetrievalError = Boolean(
    latest?.errorCode || latest?.status?.toLowerCase() === "failed",
  );

  return (
    <section className={`run-context-bar ${hasRetrievalError ? "warning" : ""}`} aria-label="服务端运行上下文">
      <span title={context.modelSpec || undefined}>
        <BrainCircuit size={14} />
        {displayModel(context.modelSpec)}
      </span>
      <span title={knowledgeNames || "本次运行未挂载知识库"}>
        <Database size={14} />
        {scope.kbCount} 个知识库
        {scope.scopeVersion != null ? ` · Scope v${scope.scopeVersion}` : ""}
      </span>
      {latest ? (
        <span title={latest.intent || "本次知识检索"}>
          {hasRetrievalError ? <TriangleAlert size={14} /> : <BookOpenCheck size={14} />}
          {latest.returnedClaimCount ?? 0} 条主张 · {latest.returnedEvidenceCount ?? 0} 条证据
        </span>
      ) : (
        <span><BookOpenCheck size={14} />尚未执行知识检索</span>
      )}
      <span className="authoritative-result">
        <ShieldCheck size={14} />Yuxi 服务端权威结果
        {context.protocolVersion ? ` · 协议 ${context.protocolVersion}` : ""}
      </span>
    </section>
  );
}
