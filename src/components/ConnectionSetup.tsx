import { useState } from "react";
import { Eye, EyeOff, KeyRound, Leaf, LoaderCircle, LockKeyhole, Server } from "lucide-react";
import { normalizeCommandError, saveConnection } from "../services/tauri-client";
import type { PublicSettings } from "../types";

type Props = {
  defaultGatewayUrl: string;
  onConnected: (settings: PublicSettings) => void;
};

export function ConnectionSetup({ defaultGatewayUrl, onConnected }: Props) {
  const [apiKey, setApiKey] = useState("");
  const [gatewayUrl, setGatewayUrl] = useState(defaultGatewayUrl);
  const [showKey, setShowKey] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    setSaving(true);
    setError("");
    try {
      const settings = await saveConnection(apiKey.trim(), gatewayUrl.trim());
      setApiKey("");
      onConnected(settings);
    } catch (reason) {
      setError(normalizeCommandError(reason).message);
    } finally {
      setSaving(false);
    }
  }

  return (
    <main className="connection-page">
      <div className="connection-decoration decoration-one" />
      <div className="connection-decoration decoration-two" />
      <section className="connection-card">
        <div className="connection-brand">
          <img src="/brand-logo.png" alt="稻芯智析徽标" />
          <div>
            <span>稻芯智析</span>
            <small>水稻胚乳科研智能体</small>
          </div>
        </div>
        <div className="connection-heading">
          <span className="connection-icon"><Leaf size={22} /></span>
          <h1>连接科研智能服务</h1>
          <p>输入你的 Yuxi API Key。凭证由系统安全存储保护，不会写入网页存储、SQLite 或日志。</p>
        </div>

        <form onSubmit={submit} className="connection-form">
          <label>
            <span><KeyRound size={16} /> API Key</span>
            <div className="secure-input">
              <input
                type={showKey ? "text" : "password"}
                value={apiKey}
                onChange={(event) => setApiKey(event.target.value)}
                placeholder="yxkey_..."
                autoComplete="off"
                spellCheck={false}
                required
              />
              <button type="button" onClick={() => setShowKey((value) => !value)} aria-label="显示或隐藏 API Key">
                {showKey ? <EyeOff size={18} /> : <Eye size={18} />}
              </button>
            </div>
          </label>

          <label>
            <span><Server size={16} /> Yuxi 网关</span>
            <input
              type="url"
              value={gatewayUrl}
              onChange={(event) => setGatewayUrl(event.target.value)}
              placeholder="https://api.example.cn"
              required
            />
            <small>远程地址必须使用 HTTPS；开发环境允许 127.0.0.1 或 localhost。</small>
          </label>

          {error && <div className="form-error" role="alert">{error}</div>}

          <button className="primary-button" disabled={saving || !apiKey.trim()}>
            {saving ? <LoaderCircle className="spin" size={18} /> : <LockKeyhole size={18} />}
            {saving ? "正在安全验证…" : "测试并安全保存"}
          </button>
        </form>
        <p className="privacy-note">测试连接不会启动模型任务，也不会产生大模型调用费用。</p>
      </section>
    </main>
  );
}
