import { useCallback, useEffect, useMemo, useRef, useState, type MouseEvent as ReactMouseEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow, PhysicalPosition } from "@tauri-apps/api/window";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { Activity, Box, BrainCircuit, Eye, ExternalLink, Gauge, Layers3, MemoryStick, Minus, Pin, RefreshCw, ServerOff, Settings2, Sparkles, X, Zap } from "lucide-react";
import type { LlamaStatus, WidgetSettings } from "./types";

const POSITION_KEY = "llama-switcher-widget.position.v1";
const SETTINGS_KEY = "llama-switcher-widget.settings.v1";
const isTauriRuntime = () => "__TAURI_INTERNALS__" in window;

const defaultSettings: WidgetSettings = {
  opacity: 90,
  blur: 24,
  refreshSeconds: 3,
  startWithWindows: false,
  alwaysOnTop: false,
  desktopMode: false,
};

function loadSettings(): WidgetSettings {
  try {
    const loaded = { ...defaultSettings, ...JSON.parse(localStorage.getItem(SETTINGS_KEY) ?? "{}") };
    return loaded.desktopMode ? { ...loaded, alwaysOnTop: false } : loaded;
  } catch {
    return defaultSettings;
  }
}

const previewStatus: LlamaStatus = {
  running: true,
  currentProfileId: "laguna-s-2-1-mini-apex__dflash",
  alias: "Laguna-S-2.1-Mini-APEX Dflash",
  currentProfileName: "Laguna-S-2.1-Mini-APEX Dflash",
  model: "Laguna-S-2.1-Mini-APEX",
  feature: "Dflash",
  pid: 31488,
  healthy: true,
  serverReachable: true,
  usageState: "busy",
  avgTokensPerSecond: 42.7,
  vram: { totalMib: 24_576, usedMib: 21_840, freeMib: 2_736, modelMib: 20_926, processes: [] },
};

function formatMemory(mib: number | null | undefined): string {
  if (mib == null) return "—";
  const gib = mib / 1024;
  return `${gib >= 10 ? gib.toFixed(1) : gib.toFixed(2)} GB`;
}

function VramRing({ percent }: { percent: number }) {
  const value = Math.max(0, Math.min(100, percent));
  const circumference = 2 * Math.PI * 62;
  return (
    <div className="vram-ring" aria-label={`${Math.round(value)} percent VRAM used`}>
      <svg viewBox="0 0 152 152" role="img">
        <defs>
          <linearGradient id="vram-gradient" x1="0" y1="0" x2="1" y2="1">
            <stop offset="0%" stopColor="#76a8ff" />
            <stop offset="55%" stopColor="#9a7cf4" />
            <stop offset="100%" stopColor="#53ddc4" />
          </linearGradient>
        </defs>
        <circle className="ring-track" cx="76" cy="76" r="62" />
        <circle className="ring-value" cx="76" cy="76" r="62" style={{ strokeDasharray: circumference, strokeDashoffset: circumference * (1 - value / 100) }} />
      </svg>
      <div className="ring-copy"><strong>{Math.round(value)}<small>%</small></strong><span>VRAM used</span></div>
    </div>
  );
}

function Toggle({ checked, onChange, label }: { checked: boolean; onChange: (next: boolean) => void; label: string }) {
  return (
    <button type="button" role="switch" aria-label={label} aria-checked={checked} className={`toggle ${checked ? "on" : ""}`} onClick={() => onChange(!checked)}>
      <span />
    </button>
  );
}

export default function App() {
  const [status, setStatus] = useState<LlamaStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [updatedAt, setUpdatedAt] = useState<Date | null>(null);
  const [settings, setSettings] = useState<WidgetSettings>(loadSettings);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [connectionState, setConnectionState] = useState<"connected" | "reconnecting" | "offline">("reconnecting");
  const refreshing = useRef(false);
  const statusRef = useRef<LlamaStatus | null>(null);
  const consecutiveFailures = useRef(0);
  const previewRequestCount = useRef(0);

  const refresh = useCallback(async () => {
    if (refreshing.current) return;
    refreshing.current = true;
    try {
      previewRequestCount.current += 1;
      const previewParams = new URLSearchParams(window.location.search);
      const previewFailure = ["offline", "reconnecting"].some((key) => previewParams.has(key))
        || (previewParams.has("flaky") && previewRequestCount.current > 1);
      if (!isTauriRuntime() && previewFailure) {
        throw new Error("Llama Switcher is not running. Start the main app to enable live telemetry.");
      }
      const next = isTauriRuntime() ? await invoke<LlamaStatus>("get_llama_status") : previewStatus;
      statusRef.current = next;
      setStatus(next);
      setUpdatedAt(new Date());
      setError(null);
      consecutiveFailures.current = 0;
      setConnectionState("connected");
    } catch (cause) {
      consecutiveFailures.current += 1;
      const detected = isTauriRuntime()
        ? await invoke<boolean>("is_switcher_running").catch(() => false)
        : ["reconnecting", "flaky"].some((key) => new URLSearchParams(window.location.search).has(key));
      setConnectionState(detected ? "reconnecting" : "offline");
      setError(
        detected
          ? "Llama Switcher is running. Reconnecting to live telemetry…"
          : String(cause).replace(/^Error:\s*/i, ""),
      );
      // Preserve the last valid snapshot through transient API stalls. Only
      // clear it after repeated failures and confirmation that the main app is
      // no longer running.
      if (!detected && consecutiveFailures.current >= 3) {
        statusRef.current = null;
        setStatus(null);
      } else if (statusRef.current) {
        setStatus(statusRef.current);
      }
    } finally {
      refreshing.current = false;
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), settings.refreshSeconds * 1_000);
    return () => window.clearInterval(timer);
  }, [refresh, settings.refreshSeconds]);

  useEffect(() => {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
    document.documentElement.style.setProperty("--panel-opacity", String(settings.opacity / 100));
    document.documentElement.style.setProperty("--panel-blur", `${settings.blur}px`);
  }, [settings]);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    const unlisten = listen("widget://refresh", () => void refresh());
    return () => { void unlisten.then((callback) => callback()); };
  }, [refresh]);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    const appWindow = getCurrentWindow();
    try {
      const position = JSON.parse(localStorage.getItem(POSITION_KEY) ?? "null");
      if (position && Number.isFinite(position.x) && Number.isFinite(position.y)) {
        void appWindow.setPosition(new PhysicalPosition(position.x, position.y));
      }
    } catch { /* Ignore stale position data. */ }
    void appWindow.setAlwaysOnBottom(settings.desktopMode);
    void appWindow.setAlwaysOnTop(settings.alwaysOnTop && !settings.desktopMode);
    void isEnabled().then((enabled) => {
      setSettings((current) => current.startWithWindows === enabled ? current : { ...current, startWithWindows: enabled });
    });
    const listener = appWindow.onMoved(({ payload }) => localStorage.setItem(POSITION_KEY, JSON.stringify(payload)));
    return () => { void listener.then((unlisten) => unlisten()); };
  }, []);

  const updateSettings = (patch: Partial<WidgetSettings>) => {
    setSettings((current) => ({ ...current, ...patch }));
  };

  const toggleAutostart = async (next: boolean) => {
    if (!isTauriRuntime()) {
      updateSettings({ startWithWindows: next });
      return;
    }
    try {
      if (next) await enable(); else await disable();
      updateSettings({ startWithWindows: next });
    } catch (cause) {
      setError(`Could not update Windows startup: ${String(cause)}`);
    }
  };

  const toggleAlwaysOnTop = async (next: boolean) => {
    if (isTauriRuntime()) {
      const appWindow = getCurrentWindow();
      if (next) await appWindow.setAlwaysOnBottom(false);
      await appWindow.setAlwaysOnTop(next);
    }
    updateSettings(next ? { alwaysOnTop: true, desktopMode: false } : { alwaysOnTop: false });
  };

  const toggleDesktopMode = async (next: boolean) => {
    if (isTauriRuntime()) {
      const appWindow = getCurrentWindow();
      if (next) await appWindow.setAlwaysOnTop(false);
      await appWindow.setAlwaysOnBottom(next);
    }
    updateSettings(next ? { desktopMode: true, alwaysOnTop: false } : { desktopMode: false });
  };

  const startWindowDrag = (event: ReactMouseEvent<HTMLElement>) => {
    if (!isTauriRuntime() || event.button !== 0) return;
    if (event.target instanceof Element && event.target.closest("button, input, select")) return;
    void getCurrentWindow().startDragging();
  };

  const openSwitcher = async () => {
    if (!isTauriRuntime()) return;
    try { await invoke("open_switcher"); }
    catch (cause) { setError(String(cause)); }
  };

  const totalMib = status?.vram.totalMib ?? 0;
  const usedMib = status?.vram.usedMib ?? 0;
  const vramPercent = totalMib > 0 ? (usedMib / totalMib) * 100 : 0;
  const isOnline = !!status?.serverReachable;
  const stateLabel = connectionState === "reconnecting"
    ? "Reconnecting"
    : connectionState === "offline"
      ? "Offline"
      : status?.usageState === "busy"
        ? "Generating"
        : status?.usageState === "free"
          ? "Ready"
          : isOnline ? "Online" : "Offline";
  const stateTone = connectionState === "reconnecting" ? "reconnecting" : status?.usageState === "busy" ? "busy" : isOnline ? "online" : "offline";
  const modelName = status?.model ?? status?.alias ?? "No model loaded";
  const updatedLabel = useMemo(() => updatedAt ? updatedAt.toLocaleTimeString([], { hour: "numeric", minute: "2-digit", second: "2-digit" }) : "Waiting", [updatedAt]);

  return (
    <main className="widget-shell">
      <section className="glass-panel">
        <div className="ambient ambient-one" /><div className="ambient ambient-two" />
        <header className="titlebar" data-tauri-drag-region onMouseDown={startWindowDrag}>
          <div className="brand" data-tauri-drag-region>
            <div className="brand-mark"><BrainCircuit size={16} /></div>
            <div data-tauri-drag-region><strong>LLAMA SWITCHER</strong><span>monitor</span></div>
          </div>
          <div className="window-actions">
            <button title="Widget settings" onClick={() => setSettingsOpen((open) => !open)}><Settings2 size={15} /></button>
            <button title="Refresh" onClick={() => void refresh()} disabled={loading}><RefreshCw size={15} className={loading ? "spin" : ""} /></button>
            <button title="Hide widget" onClick={() => isTauriRuntime() && void getCurrentWindow().hide()}><Minus size={17} /></button>
          </div>
        </header>

        {settingsOpen ? (
          <div className="settings-view">
            <div className="settings-heading">
              <div><span className="eyebrow">Appearance & behavior</span><h1>Widget settings</h1></div>
              <button className="settings-close" title="Close settings" onClick={() => setSettingsOpen(false)}><X size={16} /></button>
            </div>

            <section className="setting-card featured-setting">
              <div className="setting-title"><span><Eye size={15} /> Transparency</span><strong>{settings.opacity}%</strong></div>
              <input aria-label="Widget transparency" type="range" min="30" max="100" value={settings.opacity} onChange={(event) => updateSettings({ opacity: Number(event.target.value) })} />
              <div className="range-hints"><span>Airy</span><span>Solid</span></div>
            </section>

            <section className="setting-card">
              <div className="setting-title"><span><Sparkles size={15} /> Glass blur</span><strong>{settings.blur}px</strong></div>
              <input aria-label="Glass blur" type="range" min="0" max="36" value={settings.blur} onChange={(event) => updateSettings({ blur: Number(event.target.value) })} />
            </section>

            <section className="setting-row">
              <div><strong><Layers3 size={14} /> Desktop mode</strong><span>Keep the widget behind other windows</span></div>
              <Toggle label="Desktop mode" checked={settings.desktopMode} onChange={(next) => void toggleDesktopMode(next)} />
            </section>
            <section className="setting-row">
              <div><strong><Pin size={14} /> Always on top</strong><span>Keep telemetry above other windows</span></div>
              <Toggle label="Always on top" checked={settings.alwaysOnTop} onChange={(next) => void toggleAlwaysOnTop(next)} />
            </section>
            <section className="setting-row">
              <div><strong>Start with Windows</strong><span>Launch automatically after sign-in</span></div>
              <Toggle label="Start with Windows" checked={settings.startWithWindows} onChange={(next) => void toggleAutostart(next)} />
            </section>
            <section className="setting-row">
              <div><strong>Refresh interval</strong><span>Local telemetry polling rate</span></div>
              <select aria-label="Refresh interval" value={settings.refreshSeconds} onChange={(event) => updateSettings({ refreshSeconds: Number(event.target.value) })}>
                <option value={2}>2 sec</option><option value={3}>3 sec</option><option value={5}>5 sec</option><option value={10}>10 sec</option>
              </select>
            </section>
            <p className="settings-note">Preferences are saved automatically on this PC.</p>
          </div>
        ) : status ? (
          <div className="dashboard-view">
            <div className="status-line"><span className={`live-dot ${stateTone}`} /><span>{stateLabel}</span><span className="status-spacer" /><span>PID {status.pid ?? "—"}</span></div>

            <section className="model-card">
              <div className="model-icon"><Box size={18} /></div>
              <div className="model-copy"><span>Loaded model</span><strong title={modelName}>{modelName}</strong><small>{status.feature ?? "Standard"}</small></div>
              <Sparkles size={16} className="model-spark" />
            </section>

            <section className="vram-hero">
              <VramRing percent={vramPercent} />
              <div className="vram-copy">
                <span className="eyebrow">GPU memory</span>
                <strong>{formatMemory(usedMib)}</strong>
                <p>of {formatMemory(totalMib)} allocated</p>
                <div className="free-memory"><span>{formatMemory(status.vram.freeMib)}</span> available</div>
              </div>
            </section>

            <section className="metric-grid">
              <article><div className="metric-icon violet"><Zap size={15} /></div><span>Speed</span><strong>{status.avgTokensPerSecond != null ? status.avgTokensPerSecond.toFixed(1) : "—"}</strong><small>tokens / second</small></article>
              <article><div className="metric-icon mint"><Activity size={15} /></div><span>Usage</span><strong>{stateLabel}</strong><small>{status.healthy ? "server healthy" : "health pending"}</small></article>
              <article><div className="metric-icon blue"><MemoryStick size={15} /></div><span>Model VRAM</span><strong>{formatMemory(status.vram.modelMib)}</strong><small>{totalMib ? `${Math.round(((status.vram.modelMib ?? 0) / totalMib) * 100)}% of total` : "allocation"}</small></article>
            </section>

            <div className="memory-bar"><span style={{ width: `${vramPercent}%` }} /></div>
            <footer><span><Gauge size={11} /> Updated {updatedLabel}</span><button onClick={() => void openSwitcher()}>Open Switcher <ExternalLink size={11} /></button></footer>
          </div>
        ) : connectionState === "reconnecting" ? (
          <div className="offline-view reconnect-view">
            <div className="offline-orbit"><RefreshCw className="spin-slow" size={27} /></div>
            <span className="eyebrow">Switcher detected</span>
            <h1>Connecting to telemetry</h1>
            <p>{error ?? "Llama Switcher is running. Waiting for its local status service…"}</p>
            <button className="primary-button" onClick={() => void refresh()}><RefreshCw size={17} /> Reconnect now</button>
          </div>
        ) : (
          <div className="offline-view">
            <div className="offline-orbit"><ServerOff size={27} /></div>
            <span className="eyebrow">Switcher unavailable</span>
            <h1>No live status</h1>
            <p>{error ?? "Start Llama Switcher to see model and GPU telemetry."}</p>
            <button className="primary-button" onClick={() => void openSwitcher()}><BrainCircuit size={17} /> Open Llama Switcher</button>
            <button className="retry-button" onClick={() => void refresh()}><RefreshCw size={13} /> Try again</button>
          </div>
        )}
      </section>
    </main>
  );
}
