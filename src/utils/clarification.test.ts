import { describe, expect, it } from "vitest";
import type { ClarificationQuestion } from "../types";
import {
  OTHER_OPTION_VALUE,
  answersToDisplayText,
  draftsToAnswers,
  effectiveOptions,
  isClarificationComplete,
} from "./clarification";

const singleSelect: ClarificationQuestion = {
  questionId: "q-stage",
  question: "重点分析哪个发育阶段？",
  options: [
    { label: "灌浆期", value: "灌浆期" },
    { label: "成熟期", value: "成熟期" },
  ],
  multiSelect: false,
  allowOther: false,
};

const withOther: ClarificationQuestion = {
  questionId: "q-tissue",
  question: "关注哪种组织？",
  options: [{ label: "糊粉层", value: "aleurone" }],
  multiSelect: false,
  allowOther: true,
};

const multiSelect: ClarificationQuestion = {
  questionId: "q-omics",
  question: "需要哪些组学证据？",
  options: [
    { label: "转录组", value: "rna" },
    { label: "蛋白组", value: "protein" },
  ],
  multiSelect: true,
  allowOther: false,
};

describe("clarification utils", () => {
  it("allowOther 的问题自动追加「其他」保留选项", () => {
    const options = effectiveOptions(withOther);
    expect(options).toHaveLength(2);
    expect(options[options.length - 1]).toEqual({ label: "其他", value: OTHER_OPTION_VALUE });
    // 不重复追加
    expect(effectiveOptions({ ...withOther, options })).toHaveLength(2);
    expect(effectiveOptions(singleSelect)).toHaveLength(2);
  });

  it("完整性校验：其他必填文本，普通选项至少一项", () => {
    const questions = [singleSelect, withOther];
    expect(isClarificationComplete(questions, {})).toBe(false);
    expect(
      isClarificationComplete(questions, {
        "q-stage": { selected: ["灌浆期"], otherText: "" },
        "q-tissue": { selected: [OTHER_OPTION_VALUE], otherText: "  " },
      }),
    ).toBe(false);
    expect(
      isClarificationComplete(questions, {
        "q-stage": { selected: ["成熟期"], otherText: "" },
        "q-tissue": { selected: [OTHER_OPTION_VALUE], otherText: "胚乳外层" },
      }),
    ).toBe(true);
  });

  it("drafts 转答复保留选中与修剪后的其他文本", () => {
    const answers = draftsToAnswers([multiSelect, withOther], {
      "q-omics": { selected: ["rna", "protein"], otherText: "" },
      "q-tissue": { selected: [OTHER_OPTION_VALUE], otherText: "  淀粉胚乳  " },
    });
    expect(answers).toEqual([
      { questionId: "q-omics", selected: ["rna", "protein"], otherText: undefined },
      { questionId: "q-tissue", selected: [OTHER_OPTION_VALUE], otherText: "淀粉胚乳" },
    ]);
  });

  it("展示文本映射选项标签，其他追加「其他：文本」", () => {
    const answers = draftsToAnswers([singleSelect, withOther, multiSelect], {
      "q-stage": { selected: ["灌浆期"], otherText: "" },
      "q-tissue": { selected: [OTHER_OPTION_VALUE], otherText: "胚乳外层" },
      "q-omics": { selected: ["rna", "protein"], otherText: "" },
    });
    expect(answersToDisplayText([singleSelect, withOther, multiSelect], answers)).toBe(
      "灌浆期；其他：胚乳外层；转录组、蛋白组",
    );
  });
});
