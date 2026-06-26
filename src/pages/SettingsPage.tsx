import { useEffect, useState } from "react";
import { api } from "../api";
import type { DefaultProfileMode, Profile, Settings } from "../types";

const ALL_EXTENSIONS = [".cmd", ".bat", ".ps1"];

export function SettingsPage({
  showToast,
}: {
  showToast: (m: string, e?: boolean) => void;
}) {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    (async () => {
      try {
        setSettings(await api.getSettings());
        setProfiles(await api.getDetectedProfiles());
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

  return (
    <div>
      <h1>Settings</h1>
      <p className="subtitle">Stored as JSON in the app data directory.</p>

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
    </div>
  );
}
