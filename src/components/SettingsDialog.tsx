import { useState } from "react";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { ExternalLink, KeyRound, LoaderCircle, RefreshCw, ShieldCheck, Trash2, X } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { deleteApiKey, normalizeCommandError, testConnection } from "../services/tauri-client";
import type { PublicSettings } from "../types";

type Props = {
  settings: PublicSettings;
  onClose: () => void;
  onCredentialDeleted: () => void;
};

export function SettingsDialog({ settings, onClose, onCredentialDeleted }: Props) {
  const [status, setStatus] = useState("");
  const [busy, setBusy] = useState(false);

  async function run(action: () => Promise<void>, success: string) {
    setBusy(true);
    setStatus("");
    try {
      await action();
      setStatus(success);
    } catch (error) {
      setStatus(normalizeCommandError(error).message);
    } finally {
      setBusy(false);
    }
  }

  async function updateApp() {
    await run(async () => {
      const update = await check();
      if (!update) {
        setStatus("当前已是最新版本");
        return;
      }
      await update.downloadAndInstall();
      await relaunch();
    }, "更新已安装，正在重启");
  }

  async function removeCredential() {
    if (!window.confirm("确定删除本机 API Key 吗？历史会话不会被删除。")) return;
    await run(async () => {
      await deleteApiKey();
      onCredentialDeleted();
    }, "本机凭证已删除");
  }

  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={onClose}>
      <section className="settings-dialog" role="dialog" aria-modal="true" aria-labelledby="settings-title" onMouseDown={(event) => event.stopPropagation()}>
        <header>
          <div><span className="settings-icon"><ShieldCheck size={20} /></span><div><h2 id="settings-title">设置与连接</h2><p>本机安全配置和应用更新</p></div></div>
          <button className="icon-button" onClick={onClose} aria-label="关闭"><X size={20} /></button>
        </header>
        <div className="settings-body">
          <div className="setting-row"><span>服务地址</span><strong>{settings.gatewayUrl}</strong></div>
          <div className="setting-row"><span>智能体</span><strong>{settings.agentSlug}</strong></div>
          <div className="setting-row"><span>API Key</span><strong className="key-hint"><KeyRound size={15} /> {settings.apiKeyHint || "已安全配置"}</strong></div>
          <div className="settings-actions">
            <button onClick={() => run(testConnection, "连接正常，凭证有效")} disabled={busy}><RefreshCw size={17} /> 测试连接</button>
            <button onClick={updateApp} disabled={busy}><RefreshCw size={17} /> 检查更新</button>
            <button onClick={() => openUrl("https://github.com/yanmengli123/rice-endosperm-desktop")}><ExternalLink size={17} /> GitHub 项目</button>
            <button className="danger-button" onClick={removeCredential} disabled={busy}><Trash2 size={17} /> 删除本机凭证</button>
          </div>
          {busy && <div className="settings-status"><LoaderCircle className="spin" size={17} /> 正在处理…</div>}
          {!busy && status && <div className="settings-status">{status}</div>}
        </div>
      </section>
    </div>
  );
}
