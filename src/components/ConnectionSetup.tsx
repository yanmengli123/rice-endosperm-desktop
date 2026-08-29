import { useEffect, useRef, useState } from "react";
import { CheckCircle2, Eye, EyeOff, KeyRound, Leaf, LoaderCircle, LockKeyhole, MonitorSmartphone, Server, ShieldCheck, Ticket } from "lucide-react";
import {
  activateWithCode,
  normalizeCommandError,
  pollDeviceLogin,
  saveConnectionWithLogin,
  startDeviceLogin,
} from "../services/tauri-client";
import type { DeviceLoginStart, PublicSettings } from "../types";

type Props = {
  defaultGatewayUrl: string;
  onConnected: (settings: PublicSettings) => void;
};

export function ConnectionSetup({ defaultGatewayUrl, onConnected }: Props) {
  const [apiKey, setApiKey] = useState("");
  const [loginName, setLoginName] = useState("");
  const [loginPassword, setLoginPassword] = useState("");
  const [gatewayUrl, setGatewayUrl] = useState(defaultGatewayUrl);
  const [showKey, setShowKey] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const [deviceLogin, setDeviceLogin] = useState<DeviceLoginStart | null>(null);
  const [deviceStatus, setDeviceStatus] = useState("正在等待网页端授权…");
  const [deviceError, setDeviceError] = useState("");
  const [activationCode, setActivationCode] = useState("");
  const [activating, setActivating] = useState(false);
  const [loginMode, setLoginMode] = useState<"credentials" | "device" | "activation">("credentials");
  const pollTimer = useRef<number | undefined>(undefined);
  const pollActive = useRef(false);

  // 轮询授权结果：approved 后保存的 Key 已在 Rust 侧写入安全存储，直接进入主界面
  useEffect(() => {
    if (!deviceLogin) return;
    pollActive.current = true;
    const gateway = gatewayUrl.trim();
    const tick = async () => {
      if (!pollActive.current) return;
      try {
        const result = await pollDeviceLogin(gateway, deviceLogin.deviceCode);
        if (!pollActive.current) return;
        if (result.approved) {
          pollActive.current = false;
          setDeviceStatus(`已授权：${result.userName ?? ""}`);
          const settings = await import("../services/tauri-client").then((m) => m.getPublicSettings());
          onConnected(settings);
          return;
        }
      } catch (reason) {
        if (pollActive.current) {
          pollActive.current = false;
          setDeviceError(normalizeCommandError(reason).message);
          return;
        }
      }
      pollTimer.current = window.setTimeout(
        () => void tick(),
        Math.max(1, deviceLogin.interval) * 1000,
      );
    };
    void tick();
    return () => {
      pollActive.current = false;
      if (pollTimer.current !== undefined) window.clearTimeout(pollTimer.current);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [deviceLogin]);

  async function startLogin() {
    setError("");
    setDeviceError("");
    try {
      const keyName = `桌面端-${new Date().toLocaleDateString("zh-CN")}`;
      const start = await startDeviceLogin(gatewayUrl.trim(), keyName);
      setDeviceLogin(start);
      setDeviceStatus("正在等待网页端授权…");
      // 直接打开系统浏览器进入授权页
      void import("@tauri-apps/plugin-opener").then(({ openUrl }) =>
        openUrl(start.verificationUrl).catch(() => undefined),
      );
    } catch (reason) {
      setError(normalizeCommandError(reason).message);
    }
  }

  // P5：一次性激活码登录——管理员开户后发放，兑换纯会话凭证（无静态 Key）
  async function submitActivation(event: React.FormEvent) {
    event.preventDefault();
    setActivating(true);
    setError("");
    try {
      await activateWithCode(gatewayUrl.trim(), activationCode.trim());
      const settings = await import("../services/tauri-client").then((m) => m.getPublicSettings());
      onConnected(settings);
    } catch (reason) {
      setError(normalizeCommandError(reason).message);
    } finally {
      setActivating(false);
    }
  }

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
            <li><MonitorSmartphone size={17} /><span><strong>多种登录</strong>支持设备授权、激活码和三要素登录</span></li>
          </ul>
        </aside>

        <div className="connection-workspace">
          <header className="connection-workspace-header">
            <div><span>安全接入</span><h2>登录稻芯智析</h2></div>
            <span className="security-pill"><ShieldCheck size={14} />企业安全通道</span>
          </header>
          <nav className="connection-mode-nav" aria-label="登录方式">
            <button type="button" className={loginMode === "credentials" ? "active" : ""} onClick={() => setLoginMode("credentials")}><KeyRound size={16} />账号与 API Key</button>
            <button type="button" className={loginMode === "device" ? "active" : ""} onClick={() => setLoginMode("device")}><MonitorSmartphone size={16} />设备授权</button>
            <button type="button" className={loginMode === "activation" ? "active" : ""} onClick={() => setLoginMode("activation")}><Ticket size={16} />激活码</button>
          </nav>

          <div className="connection-mode-content">
          {loginMode === "device" && (deviceLogin ? (
            <div className="device-login-panel" role="dialog" aria-label="设备码授权">
              <MonitorSmartphone size={28} />
              <div><h3>请在网页端确认授权</h3><p className="device-login-lead">浏览器已打开授权页面，请核对并输入以下设备码：</p></div>
              <div className="device-login-code">{deviceLogin.userCode}</div>
              <p className="device-login-url">备用地址：<span>{deviceLogin.verificationUrl}</span></p>
              {deviceError ? <div className="form-error" role="alert">{deviceError}</div> : <p className="device-login-status"><LoaderCircle className="spin" size={15} /> {deviceStatus}</p>}
              <button type="button" className="secondary-button" onClick={() => { pollActive.current = false; setDeviceLogin(null); }}>取消授权登录</button>
            </div>
          ) : (
            <div className="connection-method-intro">
              <span className="method-icon"><MonitorSmartphone size={26} /></span>
              <h3>网页设备授权</h3>
              <p>推荐用于企业账号。桌面端不会接触你的网页登录密码，管理员也可随时在服务端撤销该设备。</p>
              <button type="button" className="primary-button" onClick={() => void startLogin()}><MonitorSmartphone size={18} />开始设备授权</button>
            </div>
          ))}

          {loginMode === "activation" && (
            <form onSubmit={submitActivation} className="activation-form enterprise-form">
              <div className="connection-method-intro compact"><span className="method-icon"><Ticket size={24} /></span><div><h3>一次性激活码</h3><p>适合管理员为新用户发放的首次登录，兑换后激活码立即失效。</p></div></div>
              <label><span>激活码</span><input value={activationCode} onChange={(event) => setActivationCode(event.target.value)} placeholder="yxact_..." autoComplete="off" spellCheck={false} required /><small>激活后本机绑定该账号并使用可撤销的安全设备会话。</small></label>
              {error && <div className="form-error" role="alert">{error}</div>}
              <button className="primary-button" disabled={activating || !activationCode.trim()}>{activating ? <LoaderCircle className="spin" size={18} /> : <Ticket size={18} />}{activating ? "正在激活…" : "激活并登录"}</button>
            </form>
          )}

          {loginMode === "credentials" && (
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
          )}
          </div>
          <p className="privacy-note">连接测试不会启动模型任务，也不会产生大模型调用费用。没有账号时请联系企业管理员开通。</p>
        </div>
      </section>
    </main>
  );
}
