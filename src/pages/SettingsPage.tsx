import { useEffect, useState } from "react";
import { api } from "../api";
import type { DefaultProfileMode, Profile, Settings, WidgetInstallStatus } from "../types";

const ALL_EXTENSIONS = [".cmd", ".bat", ".ps1"];

export function SettingsPage({
  showToast,
}: {
  showToast: (m: string, e?: boolean) => void;
}) {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [busy, setBusy] = useState(false);
  const [widgetStatus, setWidgetStatus] = useState<WidgetInstallStatus | null>(null);
  const [widgetInstalling, setWidgetInstalling] = useState(false);
  const [widgetPromptOpen, setWidgetPromptOpen] = useState(false);

  useEffect(() => {
    (async () => {
      try {
        const loadedSettings = await api.getSettings();
        setSettings(loadedSettings);

        // Profile choices enhance one field but should never hold up the
        // entire settings screen.
        void api.getDetectedProfiles()
          .then(setProfiles)
          .catch((error) => console.warn("Could not load profile choices", error));

        // Widget detection can touch the Windows registry. It is deliberately
        // non-blocking so the settings form is usable as soon as its own data
        // arrives.
        void api.getWidgetInstallStatus()
          .then(setWidgetStatus)
          .catch((error) => {
            console.warn("Could not determine widget installation status", error);
            setWidgetStatus({ installed: false, executablePath: null, startWithWindows: false });
          });
      } catch (e) {
        showToast(`Failed to load settings: ${String(e)}`, true);
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  if (!settings) return <div>Loading…</div>;

  function update<K extends keyof Settings>(key: K, value: Settings[K]) {
    setSettings((s) => (s ? { ...s, [key]: value } : s));
  }

  // Changing the server port keeps the health URL's port in sync, so the live
  // health probe targets the right place without extra fiddling.
  function updatePort(port: number) {
    setSettings((s) => {
      if (!s) return s;
      const healthUrl = s.healthUrl.replace(
        /(https?:\/\/[^/:]+):\d+/,
        `$1:${port}`
      );
      return { ...s, serverPort: port, healthUrl };
    });
  }

  function toggleExt(ext: string) {
    const has = settings!.allowedExtensions.includes(ext);
    update(
      "allowedExtensions",
      has
        ? settings!.allowedExtensions.filter((e) => e !== ext)
        : [...settings!.allowedExtensions, ext]
    );
  }

  async function save() {
    setBusy(true);
    try {
      const saved = await api.saveSettings(settings!);
      setSettings(saved);
      showToast("Settings saved.");
    } catch (e) {
      showToast(`Save failed: ${String(e)}`, true);
    } finally {
      setBusy(false);
    }
  }

  async function browse() {
    try {
      const folder = await api.browseFolder();
      if (folder) update("scriptsFolder", folder);
    } catch (e) {
      showToast(`Browse failed: ${String(e)}`, true);
    }
  }

  async function regenToken() {
    try {
      const token = await api.regenerateAgentApiToken();
      update("agentApiToken", token);
      showToast("New agent API token generated. Saved automatically.");
    } catch (e) {
      showToast(`Token regeneration failed: ${String(e)}`, true);
    }
  }

  async function installWidget(startWithWindows: boolean) {
    setWidgetPromptOpen(false);
    setWidgetInstalling(true);
    try {
      const installed = await api.installWidget(startWithWindows);
      setWidgetStatus(installed);
      showToast(
        startWithWindows
          ? "Widget installed and set to start with Windows."
          : "Widget installed. Windows startup is disabled."
      );
    } catch (e) {
      showToast(`Widget installation failed: ${String(e)}`, true);
    } finally {
      setWidgetInstalling(false);
    }
  }

  return (
    <div>
      <h1>Settings</h1>
      <p className="subtitle">Stored as JSON in the app data directory.</p>

      <div className="card widget-install-card">
        <div className="widget-install-copy">
          <div className="widget-install-icon" aria-hidden="true">◫</div>
          <div>
            <div className="widget-install-heading">
              <h2>Desktop widget</h2>
              <span className={`badge ${widgetStatus?.installed ? "green" : "yellow"}`}>
                {widgetStatus?.installed ? "Installed" : widgetStatus ? "Not installed" : "Checking…"}
              </span>
            </div>
            <p>
              Keep the loaded model, feature, VRAM, tokens per second, and live
              usage visible in a compact desktop monitor.
            </p>
            {widgetStatus?.installed && (
              <span className="hint">
                {widgetStatus.startWithWindows ? "Starts with Windows" : "Windows startup is off"}
              </span>
            )}
          </div>
        </div>
        <button
          className="btn primary widget-install-button"
          disabled={!widgetStatus || widgetInstalling}
          onClick={() => setWidgetPromptOpen(true)}
        >
          {widgetInstalling
            ? "Installing…"
            : widgetStatus?.installed
              ? "Reinstall / update widget"
              : "Install widget"}
        </button>
      </div>

      <div className="card">
        <h2 style={{ marginTop: 0 }}>Scripts</h2>

        <div className="field">
          <label>Scripts folder path</label>
          <div className="inline">
            <input
              type="text"
              value={settings.scriptsFolder}
              onChange={(e) => update("scriptsFolder", e.target.value)}
            />
            <button className="btn" onClick={browse}>
              Browse…
            </button>
          </div>
        </div>

        <div className="field">
          <label>File scan pattern</label>
          <input
            type="text"
            value={settings.scanPattern}
            onChange={(e) => update("scanPattern", e.target.value)}
          />
          <span className="hint">
            Must contain {"{model}"} and {"{feature}"} placeholders.
          </span>
        </div>

        <div className="field">
          <label>Allowed script extensions</label>
          <div className="checks">
            {ALL_EXTENSIONS.map((ext) => (
              <label key={ext}>
                <input
                  type="checkbox"
                  checked={settings.allowedExtensions.includes(ext)}
                  onChange={() => toggleExt(ext)}
                />
                {ext}
              </label>
            ))}
          </div>
        </div>
      </div>

      <div className="card">
        <h2 style={{ marginTop: 0 }}>Server</h2>
        <div className="field">
          <label>llama.cpp server port</label>
          <input
            type="number"
            value={settings.serverPort}
            onChange={(e) => updatePort(Number(e.target.value))}
          />
          <span className="hint">Updates the health URL port automatically.</span>
        </div>
        <div className="field">
          <label>Health URL</label>
          <input
            type="text"
            value={settings.healthUrl}
            onChange={(e) => update("healthUrl", e.target.value)}
          />
        </div>
        <div className="field">
          <label>llama.cpp API key for status probes</label>
          <input
            type="password"
            className="mono"
            value={settings.llamaServerApiKey ?? ""}
            onChange={(e) => update("llamaServerApiKey", e.target.value || null)}
            placeholder="Auto-detected from LLAMA_API_KEY when blank"
          />
          <span className="hint">
            Used only if the running profile script does not set LLAMA_API_KEY.
            Script keys are preferred so each model can use its own key.
          </span>
        </div>
        <div className="field">
          <label>Stop timeout (seconds)</label>
          <input
            type="number"
            value={settings.stopTimeoutSeconds}
            onChange={(e) => update("stopTimeoutSeconds", Number(e.target.value))}
          />
        </div>
        <div className="field">
          <label>Health check timeout (seconds)</label>
          <input
            type="number"
            value={settings.healthCheckTimeoutSeconds}
            onChange={(e) =>
              update("healthCheckTimeoutSeconds", Number(e.target.value))
            }
          />
        </div>
        <div className="field">
          <label>Server binary name(s)</label>
          <input
            type="text"
            value={settings.serverProcessNames.join(", ")}
            onChange={(e) =>
              update(
                "serverProcessNames",
                e.target.value
                  .split(",")
                  .map((n) => n.trim())
                  .filter((n) => n.length > 0)
              )
            }
          />
          <span className="hint">
            Comma-separated. Every process with one of these image names is
            killed before a new server launches, guaranteeing only one runs at a
            time. Default: <span className="mono">llama-server.exe</span>. Change
            this if your llama.cpp binary has a different name.
          </span>
        </div>
        <span className="hint">
          Start / Switch always stops the current process on this port first,
          including servers launched outside Llama Switcher.
        </span>
      </div>

      <div className="card">
        <h2 style={{ marginTop: 0 }}>Agent control API</h2>
        <div className="field">
          <label>Agent control API port</label>
          <input
            type="number"
            value={settings.agentApiPort}
            onChange={(e) => update("agentApiPort", Number(e.target.value))}
          />
          <span className="hint">Bound only to 127.0.0.1.</span>
        </div>
        <div className="field">
          <label>Agent control API token</label>
          <div className="inline">
            <input type="text" className="mono" readOnly value={settings.agentApiToken} />
            <button
              className="btn"
              onClick={() => {
                navigator.clipboard.writeText(settings.agentApiToken);
                showToast("Token copied to clipboard.");
              }}
            >
              Copy
            </button>
            <button className="btn" onClick={regenToken}>
              Regenerate
            </button>
          </div>
        </div>
      </div>

      <div className="card">
        <h2 style={{ marginTop: 0 }}>Startup &amp; scanning</h2>
        <div className="field">
          <label className="inline">
            <input
              type="checkbox"
              checked={settings.autoRescanOnStartup}
              onChange={(e) => update("autoRescanOnStartup", e.target.checked)}
            />
            Auto-rescan scripts folder on startup
          </label>
        </div>
        <div className="field">
          <label>Auto-rescan interval (seconds, blank = disabled)</label>
          <input
            type="number"
            value={settings.autoRescanIntervalSeconds ?? ""}
            onChange={(e) =>
              update(
                "autoRescanIntervalSeconds",
                e.target.value === "" ? null : Number(e.target.value)
              )
            }
          />
          <span className="hint">No polling occurs unless this is set.</span>
          {settings.defaultProfileMode !== "none" && (
            <span className="hint">
              The selected startup profile is still scanned once when Llama Switcher launches.
            </span>
          )}
        </div>

        <div className="field">
          <label>Default profile behavior</label>
          <select
            value={settings.defaultProfileMode}
            onChange={(e) =>
              update("defaultProfileMode", e.target.value as DefaultProfileMode)
            }
          >
            <option value="none">Do not auto-start anything</option>
            <option value="lastUsed">Auto-start last used profile</option>
            <option value="specific">Auto-start specific profile</option>
          </select>
        </div>

        {settings.defaultProfileMode === "specific" && (
          <div className="field">
            <label>Specific profile to auto-start</label>
            <select
              value={settings.defaultProfileId ?? ""}
              onChange={(e) =>
                update("defaultProfileId", e.target.value || null)
              }
            >
              <option value="">— select —</option>
              {profiles.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.alias}
                </option>
              ))}
            </select>
          </div>
        )}
      </div>

      <div className="btn-row">
        <button className="btn primary" disabled={busy} onClick={save}>
          {busy ? "Saving…" : "Save settings"}
        </button>
      </div>

      {widgetPromptOpen && (
        <div className="modal-backdrop" role="presentation" onMouseDown={() => setWidgetPromptOpen(false)}>
          <section
            className="widget-install-modal"
            role="dialog"
            aria-modal="true"
            aria-labelledby="widget-install-title"
            onMouseDown={(event) => event.stopPropagation()}
          >
            <div className="widget-install-modal-icon" aria-hidden="true">◫</div>
            <span className="status-kicker">Desktop widget</span>
            <h2 id="widget-install-title">Start the widget with Windows?</h2>
            <p>
              Choose whether the widget should launch automatically when you
              sign in. You can change this later from the widget’s own settings.
            </p>
            <div className="widget-install-choices">
              <button className="btn primary" onClick={() => void installWidget(true)}>
                Install &amp; start with Windows
              </button>
              <button className="btn" onClick={() => void installWidget(false)}>
                Install without startup
              </button>
              <button className="btn ghost" onClick={() => setWidgetPromptOpen(false)}>
                Cancel
              </button>
            </div>
          </section>
        </div>
      )}
    </div>
  );
}
