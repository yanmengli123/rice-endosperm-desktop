import type {
  Attachment,
  AttachmentAdapter,
  CompleteAttachment,
  PendingAttachment,
} from "@assistant-ui/react";
import { parseChatAttachment, uploadChatAttachment } from "../services/tauri-client";
import type { PendingChatAttachment } from "../types";

export const YUXI_ATTACHMENT_PART_NAME = "yuxi-chat-attachment";
const MAX_FILE_SIZE = 5 * 1024 * 1024;

function chooseParseMethod(attachment: PendingChatAttachment): string | undefined {
  if (!attachment.parseSupported || attachment.parseMethods.length === 0) return undefined;
  if (attachment.parseMethods.includes("disable")) return "disable";
  return attachment.parseMethods.find((method) => method.includes("mineru"))
    ?? attachment.parseMethods.find((method) => method.includes("rapid"))
    ?? attachment.parseMethods[0];
}

function pendingView(file: File, id: string, progress: number): PendingAttachment {
  return {
    id,
    type: file.type.startsWith("image/") ? "image" : "document",
    name: file.name,
    contentType: file.type || "application/octet-stream",
    file,
    status: { type: "running", reason: "uploading", progress },
  };
}

function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  const chunkSize = 32_768;
  for (let offset = 0; offset < bytes.length; offset += chunkSize) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + chunkSize));
  }
  return btoa(binary);
}

/**
 * assistant-ui 原生附件适配器。文件只经 Tauri IPC 交给 Rust，再由 Rust 使用
 * 当前账号的服务端凭证上传；前端不读取或持久化任何 API Key。
 */
export class YuxiAttachmentAdapter implements AttachmentAdapter {
  accept = "image/*,.pdf,.doc,.docx,.ppt,.pptx,.xls,.xlsx,.csv,.tsv,.txt,.md,.html,.json,.xml,.fasta,.fa,.fastq,.fq";

  async *add({ file }: { file: File }): AsyncGenerator<PendingAttachment, void> {
    if (file.size === 0) throw new Error("不能上传空文件");
    if (file.size > MAX_FILE_SIZE) throw new Error("单个附件不能超过 5 MB");

    const id = crypto.randomUUID();
    yield pendingView(file, id, 0.08);
    const dataBase64 = bytesToBase64(new Uint8Array(await file.arrayBuffer()));
    yield pendingView(file, id, 0.28);
    let uploaded = await uploadChatAttachment(file.name, file.type || "application/octet-stream", dataBase64);
    yield pendingView(file, id, uploaded.parseSupported ? 0.62 : 0.92);

    const parseMethod = chooseParseMethod(uploaded);
    if (parseMethod) {
      try {
        uploaded = await parseChatAttachment(uploaded, parseMethod);
        yield pendingView(file, id, 0.95);
      } catch {
        // OCR/解析是增强能力；失败时仍保留原始附件，让服务端 Agent 按文件
        // 工具能力处理，避免一次 OCR 故障阻断整个问答。
      }
    }

    yield {
      ...pendingView(file, id, 1),
      status: { type: "requires-action", reason: "composer-send" },
      content: [{ type: "data", name: YUXI_ATTACHMENT_PART_NAME, data: uploaded }],
    };
  }

  async send(attachment: PendingAttachment): Promise<CompleteAttachment> {
    return {
      ...attachment,
      status: { type: "complete" },
      content: attachment.content ?? [],
    };
  }

  async remove(_attachment: Attachment): Promise<void> {
    // 临时对象由服务端生命周期任务回收；移除只影响本次待发送集合。
  }
}
