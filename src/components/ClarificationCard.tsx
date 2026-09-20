import { useMemo, useState } from "react";
import { CircleHelp, SendHorizontal, X } from "lucide-react";
import type { ClarificationQuestion, QuestionAnswer } from "../types";
import {
  OTHER_OPTION_VALUE,
  answersToDisplayText,
  draftsToAnswers,
  effectiveOptions,
  isClarificationComplete,
  type ClarificationDrafts,
} from "../utils/clarification";

type Props = {
  runId: string;
  questions: ClarificationQuestion[];
  onSubmit: (answers: QuestionAnswer[], displayText: string) => void;
  onDismiss: () => void;
};

/** 澄清问题卡片（ask_user_question_required 中断）：
 * 与 Web 端 HumanApprovalModal 同语义——单选即点即选，多选可勾多项，
 * allowOther 时附带「其他」+ 自由文本。提交后由上层以 resume 载荷续跑父 run。 */
export function ClarificationCard({ runId, questions, onSubmit, onDismiss }: Props) {
  const [drafts, setDrafts] = useState<ClarificationDrafts>({});

  const complete = useMemo(() => isClarificationComplete(questions, drafts), [questions, drafts]);

  const toggle = (question: ClarificationQuestion, value: string) => {
    setDrafts((current) => {
      const draft = current[question.questionId] ?? { selected: [], otherText: "" };
      if (question.multiSelect) {
        const selected = draft.selected.includes(value)
          ? draft.selected.filter((item) => item !== value)
          : [...draft.selected, value];
        return { ...current, [question.questionId]: { ...draft, selected } };
      }
      // 单选：换选即覆盖；重选同一项不取消（必须始终有一个答案）。
      return { ...current, [question.questionId]: { ...draft, selected: [value] } };
    });
  };

  const setOtherText = (questionId: string, text: string) => {
    setDrafts((current) => ({
      ...current,
      [questionId]: { ...(current[questionId] ?? { selected: [], otherText: "" }), otherText: text },
    }));
  };

  const submit = () => {
    if (!complete) return;
    const answers = draftsToAnswers(questions, drafts);
    onSubmit(answers, answersToDisplayText(questions, answers));
  };

  return (
    <section className="clarification-card" role="form" aria-label="服务端澄清问题" data-run-id={runId}>
      <header className="clarification-header">
        <CircleHelp size={16} />
        <span>稻芯智析需要补充以下信息后继续分析：</span>
        <button type="button" className="clarification-dismiss" onClick={onDismiss} aria-label="放弃本次作答">
          <X size={14} />
        </button>
      </header>
      <div className="clarification-body">
        {questions.map((question) => {
          const draft = drafts[question.questionId] ?? { selected: [], otherText: "" };
          const options = effectiveOptions(question);
          const otherSelected = draft.selected.includes(OTHER_OPTION_VALUE);
          return (
            <fieldset key={question.questionId} className="clarification-question">
              <legend>{question.question}</legend>
              <div className={question.multiSelect ? "clarification-options multi" : "clarification-options"}>
                {options.map((option) => {
                  const active = draft.selected.includes(option.value);
                  return (
                    <button
                      key={option.value}
                      type="button"
                      className={active ? "clarification-option active" : "clarification-option"}
                      aria-pressed={active}
                      onClick={() => toggle(question, option.value)}
                    >
                      {option.label}
                    </button>
                  );
                })}
              </div>
              {question.allowOther && otherSelected && (
                <textarea
                  className="clarification-other"
                  rows={2}
                  placeholder="请填写具体内容…"
                  value={draft.otherText}
                  onChange={(event) => setOtherText(question.questionId, event.target.value)}
                  aria-label={`${question.question} 的其他答复`}
                />
              )}
            </fieldset>
          );
        })}
      </div>
      <footer className="clarification-footer">
        <span className="clarification-hint">作答会以续跑方式发送，原任务的上下文保持不变。</span>
        <button
          type="button"
          className="clarification-submit"
          disabled={!complete}
          onClick={submit}
        >
          <SendHorizontal size={14} />
          <span>提交并继续</span>
        </button>
      </footer>
    </section>
  );
}
