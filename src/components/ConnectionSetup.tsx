import { useEffect, useRef, useState } from "react";
import { Eye, EyeOff, KeyRound, Leaf, LoaderCircle, LockKeyhole, MonitorSmartphone, Server, Ticket } from "lucide-react";
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
  const [activationOpen, setActivationOpen] = useState(false);
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
          <p>使用管理员发放的登录 ID、初始密码和 API Key 联合登录，或在网页端完成设备码授权。凭证由系统安全存储保护，不会写入网页存储、SQLite 或日志。</p>
        </div>

        {deviceLogin ? (
          <div className="device-login-panel" role="dialog" aria-label="设备码授权">
            <MonitorSmartphone size={26} />
            <p className="device-login-lead">
              在打开的网页中确认以下授权码完成登录（账号需已由管理员开通或已自助注册）：
            </p>
            <div className="device-login-code">{deviceLogin.userCode}</div>
            <p className="device-login-url">
              若浏览器未自动打开，请访问：
              <br />
              <span>{deviceLogin.verificationUrl}</span>
            </p>
            {deviceError ? (
              <div className="form-error" role="alert">{deviceError}</div>
            ) : (
              <p className="device-login-status">
                <LoaderCircle className="spin" size={15} /> {deviceStatus}
              </p>
            )}
            <button
              type="button"
              className="secondary-button"
              onClick={() => {
                pollActive.current = false;
                setDeviceLogin(null);
              }}
            >
              取消授权登录
            </button>
          </div>
        ) : (
          <>
            <button type="button" className="device-login-entry" onClick={() => void startLogin()}>
              <MonitorSmartphone size={17} /> 使用账号登录（设备码授权）
            </button>

            <details className="activation-panel" open={activationOpen} onToggle={(event) => setActivationOpen((event.target as HTMLDetailsElement).open)}>
              <summary><Ticket size={16} /> 使用激活码登录</summary>
              <form onSubmit={submitActivation} className="activation-form">
                <label>
                  <span>激活码</span>
                  <input
                    value={activationCode}
                    onChange={(event) => setActivationCode(event.target.value)}
                    placeholder="yxact_..."
                    autoComplete="off"
                    spellCheck={false}
                    required
                  />
                  <small>管理员开户时发放的一次性激活码；激活后本机绑定该账号并使用安全会话。</small>
                </label>
                <button className="primary-button" disabled={activating || !activationCode.trim()}>
                  {activating ? <LoaderCircle className="spin" size={18} /> : <Ticket size={18} />}
                  {activating ? "正在激活…" : "激活并登录"}
                </button>
              </form>
            </details>

            <form onSubmit={submit} className="connection-form">
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
            <p className="privacy-note">测试连接不会启动模型任务，也不会产生大模型调用费用。没有账号？请联系管理员开通，或在网页端开放自助注册后自行注册。</p>
          </>
        )}
      </section>
    </main>
  );
}
