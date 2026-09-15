import { useEffect, useRef, useState } from "react";
import {
  CheckCircle2,
  Cpu,
  ExternalLink,
  Eye,
  EyeOff,
  KeyRound,
  Leaf,
  LoaderCircle,
  LockKeyhole,
  ScanLine,
  Server,
  ShieldCheck,
} from "lucide-react";
import {
  activateEnterpriseAccount,
  beginDeviceLogin,
  normalizeCommandError,
  openAuthorizationPage,
  pollDeviceLogin,
  saveConnectionWithLogin,
} from "../services/tauri-client";
import type { DeviceCodeStart, PublicSettings } from "../types";

type Props = {
  defaultGatewayUrl: string;
  onConnected: (settings: PublicSettings) => void;
  onOpenWorkflow?: () => void;
};

type LoginMode = "activation" | "device" | "legacy";

export function ConnectionSetup({ defaultGatewayUrl, onConnected, onOpenWorkflow }: Props) {
  const [mode, setMode] = useState<LoginMode>("activation");
  const [gatewayUrl, setGatewayUrl] = useState(defaultGatewayUrl);

  // 企业激活码
  const [activationCode, setActivationCode] = useState("");
  const [deviceName, setDeviceName] = useState("");
  const [showCode, setShowCode] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  // 设备码浏览器授权
  const [deviceStart, setDeviceStart] = useState<DeviceCodeStart | null>(null);
  const [devicePolling, setDevicePolling] = useState(false);
  const [deviceExpiresAt, setDeviceExpiresAt] = useState(0);
  const [nowTick, setNowTick] = useState(Date.now());
  const [deviceError, setDeviceError] = useState("");
  const deviceTimerRef = useRef<number | undefined>(undefined);
  const countdownRef = useRef<number | undefined>(undefined);

  // 兼容模式三要素
  const [apiKey, setApiKey] = useState("");
  const [loginName, setLoginName] = useState("");
  const [loginPassword, setLoginPassword] = useState("");
  const [showKey, setShowKey] = useState(false);

  useEffect(
    () => () => {
      if (deviceTimerRef.current) window.clearInterval(deviceTimerRef.current);
      if (countdownRef.current) window.clearInterval(countdownRef.current);
    },
    [],
  );

  function stopDevicePolling() {
    if (deviceTimerRef.current) window.clearInterval(deviceTimerRef.current);
    if (countdownRef.current) window.clearInterval(countdownRef.current);
    deviceTimerRef.current = undefined;
    countdownRef.current = undefined;
    setDevicePolling(false);
  }

  function switchMode(next: LoginMode) {
    setMode(next);
    setError("");
    setDeviceError("");
    stopDevicePolling();
  }

  async function submitActivation(event: React.FormEvent) {
    event.preventDefault();
    const code = activationCode.trim();
    if (!code) {
      setError("请输入管理员发放的一次性激活码（yxact_ 开头）");
      return;
    }
    setSaving(true);
    setError("");
    try {
      const settings = await activateEnterpriseAccount(
        code,
        gatewayUrl.trim(),
        deviceName.trim() || undefined,
      );
      setActivationCode("");
      onConnected(settings);
    } catch (reason) {
      setError(normalizeCommandError(reason).message);
    } finally {
      setSaving(false);
    }
  }

  async function submitLegacy(event: React.FormEvent) {
    event.preventDefault();
    if (!loginName.trim() || !loginPassword) {
      setError("请填写管理员发放的登录名与初始密码");
      return;
    }
    if (!apiKey.trim()) {
      setError("兼容模式需要同时提供 API Key（yxkey_ 开头）");
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

  async function startDeviceLogin() {
    setDeviceError("");
    setSaving(true);
    try {
      const start = await beginDeviceLogin(gatewayUrl.trim());
      setDeviceStart(start);
      setDeviceExpiresAt(Date.now() + start.expiresIn * 1000);
      setDevicePolling(true);
      // 打开浏览器授权页；同时本地展示 user_code 供用户在网页上核对，
      // 防止 verification_uri_complete 预填被钓鱼页冒用。
      await openAuthorizationPage(start.verificationUriComplete).catch((reason) => {
        setDeviceError(
          `浏览器授权页未能自动打开（${normalizeCommandError(reason).message}），请手动复制授权地址`,
        );
      });
      if (countdownRef.current) window.clearInterval(countdownRef.current);
      countdownRef.current = window.setInterval(() => {
        setNowTick(Date.now());
      }, 1000);
      if (deviceTimerRef.current) window.clearInterval(deviceTimerRef.current);
      const pollOnce = async () => {
        try {
          const poll = await pollDeviceLogin(start.deviceCode, gatewayUrl.trim());
          if (poll.status === "done" && poll.settings) {
            stopDevicePolling();
            setDeviceStart(null);
            onConnected(poll.settings);
            return;
          }
          // authorization_pending：继续按 interval 轮询，不是错误
        } catch (reason) {
          const normalized = normalizeCommandError(reason);
          stopDevicePolling();
          setDeviceStart(null);
          // 410 授权会话过期等终态：停止并提示重新发起
          setDeviceError(normalized.message);
        }
      };
      deviceTimerRef.current = window.setInterval(() => void pollOnce(), start.interval * 1000);
      void pollOnce();
    } catch (reason) {
      setDeviceError(normalizeCommandError(reason).message);
      setDeviceStart(null);
    } finally {
      setSaving(false);
    }
  }

  const deviceSecondsLeft = Math.max(
    0,
    Math.floor((deviceExpiresAt - nowTick) / 1000),
  );

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
            <p>账号、模型和知识范围均由 Yuxi 服务端统一管理；推荐使用企业激活码开通，本机仅保存受系统安全存储保护的会话凭证。</p>
          </div>
          <ul className="connection-assurances">
            <li><ShieldCheck size={17} /><span><strong>凭证隔离</strong>不写入浏览器存储、SQLite 或日志</span></li>
            <li><CheckCircle2 size={17} /><span><strong>服务端权威</strong>问答、知识范围和模型策略完全一致</span></li>
            <li><KeyRound size={17} /><span><strong>会话优先</strong>短时访问令牌自动轮换，无长期静态密钥</span></li>
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

          <div className="connection-tabs" role="tablist" aria-label="登录方式">
            <button
              role="tab"
              aria-selected={mode === "activation"}
              className={mode === "activation" ? "active" : ""}
              onClick={() => switchMode("activation")}
              type="button"
            >
              <KeyRound size={16} />企业激活码
            </button>
            <button
              role="tab"
              aria-selected={mode === "device"}
              className={mode === "device" ? "active" : ""}
              onClick={() => switchMode("device")}
              type="button"
            >
              <ScanLine size={16} />浏览器授权
            </button>
            <button
              role="tab"
              aria-selected={mode === "legacy"}
              className={mode === "legacy" ? "active" : ""}
              onClick={() => switchMode("legacy")}
              type="button"
            >
              <LockKeyhole size={16} />账号密码<small className="legacy-tag">兼容模式</small>
            </button>
          </div>

          <div className="connection-mode-content">
            {mode === "activation" && (
              <form onSubmit={submitActivation} className="connection-form enterprise-form">
                <div className="form-section-heading">
                  <div>
                    <h3>管理员发放的一次性激活码</h3>
                    <p>激活码在服务端兑换为设备会话（不产生长期 API Key），一次性使用，兑换后即失效。</p>
                  </div>
                </div>
                <label>
                  <span><KeyRound size={16} /> 激活码</span>
                  <div className="secure-input">
                    <input
                      type={showCode ? "text" : "password"}
                      value={activationCode}
                      onChange={(event) => setActivationCode(event.target.value)}
                      placeholder="yxact_..."
                      autoComplete="off"
                      spellCheck={false}
                      required
                    />
                    <button type="button" onClick={() => setShowCode((value) => !value)} aria-label="显示或隐藏激活码">
                      {showCode ? <EyeOff size={18} /> : <Eye size={18} />}
                    </button>
                  </div>
                </label>
                <label>
                  <span>设备备注名（可选）</span>
                  <input
                    value={deviceName}
                    onChange={(event) => setDeviceName(event.target.value)}
                    placeholder="例如：实验室台式机"
                    autoComplete="off"
                    maxLength={100}
                  />
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

                <button className="primary-button" disabled={saving || !activationCode.trim()}>
                  {saving ? <LoaderCircle className="spin" size={18} /> : <ShieldCheck size={18} />}
                  {saving ? "正在安全激活…" : "激活并安全保存"}
                </button>
              </form>
            )}

            {mode === "device" && (
              <div className="connection-form enterprise-form">
                <div className="form-section-heading">
                  <div>
                    <h3>浏览器授权登录</h3>
                    <p>已有服务端账号的用户经网页批准后，本机获得与网页同源的安全会话。</p>
                  </div>
                </div>
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

                {!deviceStart && (
                  <button
                    className="primary-button"
                    onClick={() => void startDeviceLogin()}
                    disabled={saving || devicePolling}
                    type="button"
                  >
                    {saving ? <LoaderCircle className="spin" size={18} /> : <ScanLine size={18} />}
                    {saving ? "正在创建授权会话…" : "开始浏览器授权"}
                  </button>
                )}

                {deviceStart && (
                  <div className="device-code-panel" role="status">
                    <p className="device-code-label">请在打开的授权页中核对此确认码：</p>
                    <p className="device-code-value">{deviceStart.userCode}</p>
                    <p className="device-code-hint">
                      {devicePolling
                        ? `等待浏览器批准…（剩余 ${deviceSecondsLeft} 秒，过期请重新发起）`
                        : "轮询已停止，请重新发起授权"}
                    </p>
                    <div className="device-code-actions">
                      <button
                        type="button"
                        onClick={() => void openAuthorizationPage(deviceStart.verificationUriComplete)}
                      >
                        <ExternalLink size={15} />重新打开授权页
                      </button>
                      <button type="button" className="danger-button" onClick={() => {
                        stopDevicePolling();
                        setDeviceStart(null);
                      }}>
                        取消
                      </button>
                    </div>
                  </div>
                )}

                {(deviceError || (mode === "device" && error)) && (
                  <div className="form-error" role="alert">{deviceError || error}</div>
                )}
              </div>
            )}

            {mode === "legacy" && (
              <form onSubmit={submitLegacy} className="connection-form enterprise-form">
                <div className="form-section-heading">
                  <div>
                    <h3>管理员发放的登录凭据<span className="legacy-tag">兼容模式</span></h3>
                    <p>三项凭据会在服务端进行原子校验，任一不匹配都不会绑定本机。此方式依赖长期 API Key，建议升级为激活码或浏览器授权。</p>
                  </div>
                </div>
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
