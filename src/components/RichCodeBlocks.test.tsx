// @vitest-environment jsdom

import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup } from "@testing-library/react";
import { fireEvent, render, screen } from "@testing-library/react";
import { HtmlFencedCodeBlock } from "./RichCodeBlocks";

const CARD_HTML = `<div style="font-family: sans-serif; padding:4px 8px;">
  <div style="font-size:13px; font-weight:600;">胚乳发育关键调控基因（10 个唯一基因）</div>
  <div style="color:#2c5021;">FLO7 · OsNF-YB1 · OsGCD1</div>
</div>`;

describe("HtmlFencedCodeBlock", () => {
  afterEach(cleanup);
  it("默认渲染沙箱预览而非源码，且剥离脚本", () => {
    const { container } = render(
      <HtmlFencedCodeBlock code={`<script>window.evil=1</script>${CARD_HTML}`} />,
    );
    // 预览 iframe 存在（源码 Prism 视图不存在）
    expect(container.querySelector("iframe.html-preview-frame")).toBeTruthy();
    expect(container.querySelector("pre.syntax-code-block")).toBeNull();
    // 脚本标签被 DOMPurify 剥离（iframe srcDoc 内不可见）
    const frame = container.querySelector("iframe.html-preview-frame") as HTMLIFrameElement;
    expect(frame.getAttribute("srcdoc") ?? "").not.toContain("<script>");
    // 头部带"查看源码"切换
    expect(screen.getByRole("button", { name: "切换到源码视图" })).toBeTruthy();
  });

  it("点击查看源码切换到 Prism 视图，再点切回渲染", () => {
    const { container } = render(<HtmlFencedCodeBlock code={CARD_HTML} />);
    fireEvent.click(screen.getByRole("button", { name: "切换到源码视图" }));
    expect(container.querySelector("pre.syntax-code-block")).toBeTruthy();
    expect(container.querySelector("iframe.html-preview-frame")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "切换到渲染视图" }));
    expect(container.querySelector("iframe.html-preview-frame")).toBeTruthy();
  });

  it("复制按钮把源码写入剪贴板", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    render(<HtmlFencedCodeBlock code={CARD_HTML} />);
    fireEvent.click(screen.getByRole("button", { name: "复制代码" }));
    await vi.waitFor(() => expect(writeText).toHaveBeenCalled());
    expect(writeText.mock.calls[0][0]).toContain("胚乳发育关键调控基因");
  });
});
