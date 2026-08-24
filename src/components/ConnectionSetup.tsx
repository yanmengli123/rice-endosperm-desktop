import { useEffect, useRef, useState } from "react";
import { Eye, EyeOff, KeyRound, Leaf, LoaderCircle, LockKeyhole, MonitorSmartphone, Server } from "lucide-react";
import {
  normalizeCommandError,
  pollDeviceLogin,
  saveConnection,
  startDeviceLogin,
} from "../services/tauri-client";
import type { DeviceLoginStart, PublicSettings } from "../types";

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
  const [deviceLogin, setDeviceLogin] = useState<DeviceLoginStart | null>(null);
  const [deviceStatus, setDeviceStatus] = useState("正在等待网页端授权…");
  const [deviceError, setDeviceError] = useState("");
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
          <p>输入你的 Yuxi API Key，或使用账号在网页端一键授权。凭证由系统安全存储保护，不会写入网页存储、SQLite 或日志。</p>
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
