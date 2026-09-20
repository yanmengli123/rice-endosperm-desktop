import type { ClarificationQuestion, QuestionAnswer, QuestionOption } from "../types";

/** 「其他」选项保留值，与 Web 端 DEFAULT_OTHER_OPTION_VALUE、Rust 侧
 * yuxi::OTHER_OPTION_VALUE 同源——三处任意一处改动都会破坏 resume 载荷语义。 */
export const OTHER_OPTION_VALUE = "__other__";

/** 单个问题的作答草稿（卡片本地状态）。 */
export type QuestionDraft = {
  selected: string[];
  otherText: string;
};

export type ClarificationDrafts = Record<string, QuestionDraft>;

export function effectiveOptions(question: ClarificationQuestion): QuestionOption[] {
  const options = [...question.options];
  if (question.allowOther && !options.some((option) => option.value === OTHER_OPTION_VALUE)) {
    options.push({ label: "其他", value: OTHER_OPTION_VALUE });
  }
  return options;
}

export function isDraftComplete(draft: QuestionDraft | undefined): boolean {
  if (!draft) return false;
  const otherSelected = draft.selected.includes(OTHER_OPTION_VALUE);
  if (otherSelected) return draft.otherText.trim().length > 0;
  return draft.selected.length > 0;
}

export function isClarificationComplete(
  questions: ClarificationQuestion[],
  drafts: ClarificationDrafts,
): boolean {
  return questions.every((question) => isDraftComplete(drafts[question.questionId]));
}

export function draftsToAnswers(
  questions: ClarificationQuestion[],
  drafts: ClarificationDrafts,
): QuestionAnswer[] {
  return questions.map((question) => {
    const draft = drafts[question.questionId] ?? { selected: [], otherText: "" };
    return {
      questionId: question.questionId,
      selected: [...draft.selected],
      otherText: draft.otherText.trim() ? draft.otherText.trim() : undefined,
    };
  });
}

/** 组装提交后展示在会话流里的用户消息文本（仅展示用途，不参与 resume 载荷）。 */
export function answersToDisplayText(
  questions: ClarificationQuestion[],
  answers: QuestionAnswer[],
): string {
  const parts: string[] = [];
  for (const question of questions) {
    const answer = answers.find((item) => item.questionId === question.questionId);
    if (!answer) continue;
    const labels = answer.selected
      .filter((value) => value !== OTHER_OPTION_VALUE)
      .map((value) => question.options.find((option) => option.value === value)?.label ?? value);
    const other = answer.otherText?.trim();
    const chosen = other ? [...labels, `其他：${other}`] : labels;
    if (chosen.length > 0) parts.push(chosen.join("、"));
  }
  return parts.join("；");
}
