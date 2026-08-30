import { useState } from "react";
import { CheckCircle2, Cpu, Eye, EyeOff, KeyRound, Leaf, LoaderCircle, LockKeyhole, Server, ShieldCheck } from "lucide-react";
import {
  normalizeCommandError,
  saveConnectionWithLogin,
} from "../services/tauri-client";
import type { PublicSettings } from "../types";

type Props = {
  defaultGatewayUrl: string;
  onConnected: (settings: PublicSettings) => void;
  onOpenWorkflow?: () => void;
};

export function ConnectionSetup({ defaultGatewayUrl, onConnected, onOpenWorkflow }: Props) {
  const [apiKey, setApiKey] = useState("");
  const [loginName, setLoginName] = useState("");
  const [loginPassword, setLoginPassword] = useState("");
  const [gatewayUrl, setGatewayUrl] = useState(defaultGatewayUrl);
  const [showKey, setShowKey] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    if (!loginName.trim() || !loginPassword) {
      setError("请填写管理员发放的登录名与初始密码");
      return;
    }
    setSaving(true);
    setError("");
    try {
      // P5 三字段登录：服务端原子校验登录标识、密码与密钥归属后才绑定本机
      const settings = await saveConnectionWithLogin(
        apiKey.trim(),
        gatewayUrl.trim(),
        loginName.trim(),
        loginPassword,
      );
      setApiKey("");
      setLoginPassword("");
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
      <section className="connection-shell">
        <aside className="connection-rail">
          <div className="connection-brand">
            <img src="/brand-logo.png" alt="稻芯智析徽标" />
            <div><span>稻芯智析</span><small>水稻胚乳科研智能体</small></div>
          </div>
          <div className="connection-rail-copy">
            <span className="connection-icon"><Leaf size={22} /></span>
            <p className="eyebrow">ENTERPRISE RESEARCH AI</p>
            <h1>连接你的科研工作空间</h1>
            <p>账号、模型和知识范围均由 Yuxi 服务端统一管理；桌面端仅保存受系统安全存储保护的会话凭证。</p>
          </div>
          <ul className="connection-assurances">
            <li><ShieldCheck size={17} /><span><strong>凭证隔离</strong>不写入浏览器存储、SQLite 或日志</span></li>
            <li><CheckCircle2 size={17} /><span><strong>服务端权威</strong>问答、知识范围和模型策略完全一致</span></li>
            <li><KeyRound size={17} /><span><strong>统一登录</strong>账号与 API Key 经服务端原子校验</span></li>
          </ul>
          {onOpenWorkflow && (
            <button className="connection-local-workflow" onClick={onOpenWorkflow}>
              <Cpu size={17} /><span><strong>无需登录</strong>进入本地科研工作流</span>
            </button>
          )}
        </aside>

        <div className="connection-workspace">
          <header className="connection-workspace-header">
            <div><span>安全接入</span><h2>登录稻芯智析</h2></div>
            <span className="security-pill"><ShieldCheck size={14} />企业安全通道</span>
          </header>

          <div className="connection-mode-content">
            <form onSubmit={submit} className="connection-form enterprise-form">
              <div className="form-section-heading"><div><h3>管理员发放的登录凭据</h3><p>三项凭据会在服务端进行原子校验，任一不匹配都不会绑定本机。</p></div></div>
              <label>
                <span>登录名</span>
                <input
                  value={loginName}
                  onChange={(event) => setLoginName(event.target.value)}
                  placeholder="管理员发放的登录 ID"
                  autoComplete="username"
                  required
                />
              </label>
              <label>
                <span>初始密码</span>
                <input
                  type="password"
                  value={loginPassword}
                  onChange={(event) => setLoginPassword(event.target.value)}
                  placeholder="管理员发放的初始密码"
                  autoComplete="current-password"
                  required
                />
              </label>
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
          </div>
          <p className="privacy-note">连接测试不会启动模型任务，也不会产生大模型调用费用。没有账号时请联系企业管理员开通。</p>
        </div>
      </section>
    </main>
  );
}
