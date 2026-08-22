// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { ServerRunContext } from "../types";
import { RunContextBar } from "./RunContextBar";

const context: ServerRunContext = {
  protocolVersion: "1.1",
  modelSpec: "deepseek:deepseek-v4-flash",
  knowledgeScope: {
    allowWeb: false,
    kbCount: 1,
    members: [
      {
        kbId: "kb-1",
        kbName: "水稻胚乳发育",
        documentEnabled: true,
        graphEnabled: true,
        structuredEnabled: true,
      },
    ],
  },
  knowledgeRetrievals: [
    {
      status: "FAILED",
      sourceStatus: [],
      warnings: [],
    },
  ],
};

describe("RunContextBar", () => {
  it("treats server retrieval statuses as case-insensitive", () => {
    render(<RunContextBar context={context} />);
    expect(screen.getByLabelText("服务端运行上下文")).toHaveClass("warning");
  });
});
