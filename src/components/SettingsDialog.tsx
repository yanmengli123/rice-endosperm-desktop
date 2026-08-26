import { useEffect, useState } from "react";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { ExternalLink, KeyRound, LoaderCircle, RefreshCw, ShieldCheck, Trash2, X } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  deleteApiKey,
  getChatModelPreference,
  listAccounts,
  listByokCredentials,
  listChatModels,
  normalizeCommandError,
  removeAccount,
  removeByokCredential,
  saveByokCredential,
  switchAccount,
  setChatModelPreference,
  testConnection,
} from "../services/tauri-client";
import type { ByokCredential, ModelOption, PublicSettings } from "../types";
import type { AccountSummary } from "../services/tauri-client";

type Props = {
  settings: PublicSettings;
  onClose: () => void;
  onCredentialDeleted: () => void;
};

export function SettingsDialog({ settings, onClose, onCredentialDeleted }: Props) {
  const [status, setStatus] = useState("");
  const [busy, setBusy] = useState(false);
  const [models, setModels] = useState<ModelOption[]>([]);
  const [modelSpec, setModelSpec] = useState<string>("");
  const [modelState, setModelState] = useState<"loading" | "ready" | "error">("loading");
  const [modelSaving, setModelSaving] = useState(false);
  const [accounts, setAccounts] = useState<AccountSummary[]>([]);
  // P5 BYOK：自有模型密钥管理（服务端加密存储，本机仅经 Rust 请求发送，不落盘）
  const [byokList, setByokList] = useState<ByokCredential[]>([]);
  const [byokProvider, setByokProvider] = useState("");
  const [byokKey, setByokKey] = useState("");

  const byokProviders = Array.from(new Set(models.map((model) => model.spec.split(":")[0])));

  async function reloadByok() {
    const rows = await listByokCredentials();
    setByokList(rows.filter((row) => row.status === "active"));
  }

  async function saveByok() {
    if (!byokProvider || !byokKey.trim()) {
      setStatus("请选择供应商并填写 API Key");
      return;
    }
    setBusy(true);
    try {
      await saveByokCredential(byokProvider, byokKey.trim());
      setByokKey("");
      setStatus(`已保存 ${byokProvider} 的自有密钥；该供应商的对话将优先使用你的密钥`);
      await reloadByok();
    } catch (error) {
      setStatus(normalizeCommandError(error).message);
    } finally {
      setBusy(false);
    }
  }

  async function removeByok(credentialId: number) {
    if (!window.confirm("移除后使用该密钥的对话将回到企业模型轨道，确定吗？")) return;
    setBusy(true);
    try {
      await removeByokCredential(credentialId);
      await reloadByok();
      setStatus("已移除自有密钥");
    } catch (error) {
      setStatus(normalizeCommandError(error).message);
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const rows = await listAccounts();
        if (cancelled) return;
        setAccounts(rows);
      } catch {
        // 账号目录读取失败不打断设置页，仅不展示切换区
      }
      try {
        const rows = await listByokCredentials();
        if (cancelled) return;
        setByokList(rows.filter((row) => row.status === "active"));
      } catch {
        // BYOK 加载失败不打断设置页
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  async function handleSwitch(accountScope: string) {
    setBusy(true);
    try {
      await switchAccount(accountScope);
      setAccounts((rows) => rows.map((row) => ({ ...row, isActive: row.accountScope === accountScope })));
      setStatus("已切换账号，正在刷新连接…");
      onCredentialDeleted(); // 复用既有"凭据变化"回调触发全局重载
      setStatus("已切换账号");
    } catch (error) {
      setStatus(normalizeCommandError(error).message);
    } finally {
      setBusy(false);
    }
  }

  async function handleRemove(accountScope: string) {
    if (!window.confirm("移除该账号的本地登录信息？历史会话仍保留在本机。")) return;
    setBusy(true);
    try {
      await removeAccount(accountScope);
      setAccounts((rows) => rows.filter((row) => row.accountScope !== accountScope));
      setStatus("已移除该账号");
    } catch (error) {
      setStatus(normalizeCommandError(error).message);
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const [items, saved] = await Promise.all([listChatModels(), getChatModelPreference()]);
        if (cancelled) return;
        setModels(items);
        setModelSpec(saved ?? "");
        setModelState("ready");
      } catch (error) {
        if (cancelled) return;
        setModelState("error");
        setStatus(`模型偏好加载失败：${normalizeCommandError(error).message}`);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  async function changeModel(spec: string) {
    const previousSpec = modelSpec;
    setModelSpec(spec);
    setModelSaving(true);
    try {
      await setChatModelPreference(spec || undefined);
      setStatus(spec ? `默认模型已设为 ${spec}` : "已清除默认模型偏好");
    } catch (error) {
      setModelSpec(previousSpec);
      setStatus(normalizeCommandError(error).message);
    } finally {
      setModelSaving(false);
    }
  }

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
    setBusy(true);
    setStatus("");
    try {
      const update = await check();
      if (!update) {
        setStatus("当前已是最新版本");
        return;
      }
      await update.downloadAndInstall();
      // 先给出可见提示再重启：relaunch 会立刻结束进程，成功提示放在
      // relaunch 之后用户永远看不到，容易误以为更新失败。
      setStatus("更新已安装，正在重启…");
      await new Promise((resolve) => setTimeout(resolve, 800));
      await relaunch();
    } catch (error) {
      setStatus(normalizeCommandError(error).message);
    } finally {
      setBusy(false);
    }
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
          <div className="setting-row setting-row-stack">
            <span>账号默认模型<span className="model-hint">（同步保存到 Yuxi 服务端）</span></span>
            <select
              className="model-select"
              value={modelSpec}
              onChange={(event) => void changeModel(event.target.value)}
              aria-label="选择默认聊天模型"
              disabled={modelState !== "ready" || modelSaving}
            >
              <option value="">
                {modelState === "loading"
                  ? "正在加载模型偏好…"
                  : modelState === "error"
                    ? "模型偏好暂不可用"
                    : "跟随智能体/系统默认"}
              </option>
              {models.map((model) => (
                <option key={model.spec} value={model.spec}>{model.label}</option>
              ))}
            </select>
          </div>
          {accounts.length > 0 && (
            <div className="settings-field">
              <span>本机已登录账号</span>
              <ul className="account-list">
                {accounts.map((account) => (
                  <li key={account.accountScope} className="account-row">
                    <span className="account-name">
                      {account.displayName || account.accountScope}
                      {account.isActive && <em className="account-active">当前</em>}
                    </span>
                    {!account.isActive && (
                      <span className="account-actions">
                        <button onClick={() => void handleSwitch(account.accountScope)} disabled={busy}>
                          切换
                        </button>
                        <button
                          className="danger-button"
                          onClick={() => void handleRemove(account.accountScope)}
                          disabled={busy}
                        >
                          移除
                        </button>
                      </span>
                    )}
                  </li>
                ))}
              </ul>
            </div>
          )}
          {byokProviders.length > 0 && (
            <div className="settings-field setting-row-stack">
              <span>我的模型密钥<span className="model-hint">（BYOK：自带厂商密钥，服务端加密保存，仅显示掩码）</span></span>
              <ul className="account-list">
                {byokList.map((cred) => (
                  <li key={cred.credentialId} className="account-row">
                    <span className="account-name">
                      {cred.providerId} · {cred.label || "自有密钥"} · {cred.maskedHint}
                    </span>
                    <span className="account-actions">
                      <button className="danger-button" onClick={() => void removeByok(cred.credentialId)} disabled={busy}>
                        移除
                      </button>
                    </span>
                  </li>
                ))}
              </ul>
              <div className="byok-form">
                <select
                  className="model-select"
                  value={byokProvider}
                  onChange={(event) => setByokProvider(event.target.value)}
                  aria-label="选择供应商"
                  disabled={busy}
                >
                  <option value="">选择供应商…</option>
                  {byokProviders.map((provider) => (
                    <option key={provider} value={provider}>{provider}</option>
                  ))}
                </select>
                <input
                  type="password"
                  value={byokKey}
                  onChange={(event) => setByokKey(event.target.value)}
                  placeholder="粘贴你在厂商处购买的 API Key"
                  autoComplete="off"
                  spellCheck={false}
                  disabled={busy}
                />
                <button onClick={() => void saveByok()} disabled={busy || !byokProvider || !byokKey.trim()}>
                  保存
                </button>
              </div>
            </div>
          )}
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
