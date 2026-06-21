import { useEffect, useState } from "react";
import { api } from "../api";
import type { AgentApiInfo } from "../types";

export function AgentControlPage({
  showToast,
}: {
  showToast: (m: string, e?: boolean) => void;
}) {
  const [info, setInfo] = useState<AgentApiInfo | null>(null);
  const [installing, setInstalling] = useState(false);

  useEffect(() => {
    api.getAgentApiInfo().then(setInfo).catch((e) => showToast(String(e), true));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function installSkill() {
    setInstalling(true);
    try {
      // Auto-detect the active Hermes profile; fall back to letting the user browse.
      const detected = await api.detectHermesSkillDirs();
      let target: string | null = detected[0] ?? null;
      if (!target) {
        showToast("No Hermes profile detected — pick its .hermes or skills folder.");
        target = await api.browseFolder();
      }
      if (!target) {
        showToast("Install cancelled.");
        return;
      }
      const dest = await api.installHermesSkill(target);
      showToast(`Hermes skill installed at ${dest}. Start a new Hermes session or use /reset.`);
    } catch (e) {
      showToast(`Skill install failed: ${String(e)}`, true);
    } finally {
      setInstalling(false);
    }
  }

  if (!info) return <div>Loading…</div>;

  const copy = (text: string, label: string) => {
    navigator.clipboard.writeText(text);
    showToast(`${label} copied.`);
  };

  const curlStatus = `curl ${info.baseUrl}/status ^\n  -H "Authorization: Bearer ${info.token}"`;
  const curlSwitch = `curl -X POST ${info.baseUrl}/switch-by-alias ^\n  -H "Authorization: Bearer ${info.token}" ^\n  -H "Content-Type: application/json" ^\n  -d "{\\"alias\\":\\"Qwen-27B MTP\\"}"`;

  return (
    <div>
      <h1>Agent Control</h1>
      <p className="subtitle">
        Local HTTP API for Hermes Agent. Bound to 127.0.0.1 only. All endpoints
        except <span className="mono">/health</span> require the bearer token.
      </p>

      <div className="card">
        <div className="row">
          <span className="label">Base URL</span>
          <span className="value mono">{info.baseUrl}</span>
        </div>
        <div className="row">
          <span className="label">Port</span>
          <span className="value mono">{info.port}</span>
        </div>
        <div className="row">
          <span className="label">API token</span>
          <span className="value mono">{info.token}</span>
        </div>
        <div className="btn-row">
          <button className="btn" onClick={() => copy(info.baseUrl, "Base URL")}>
            Copy base URL
          </button>
          <button className="btn" onClick={() => copy(info.token, "Token")}>
            Copy token
          </button>
        </div>
      </div>

      <h2>Example curl tests</h2>
      <pre className="code-block">{curlStatus}</pre>
      <pre className="code-block">{curlSwitch}</pre>

      <h2>Hermes skill</h2>
      <p className="subtitle">
        Install the native <span className="mono">SKILL.md</span> into your
        active Hermes profile and securely add the API settings to that
        profile's <span className="mono">.env</span>. If detection fails,
        choose the profile's <span className="mono">.hermes</span> or{" "}
        <span className="mono">skills</span> folder.
      </p>
      <div className="btn-row">
        <button className="btn primary" disabled={installing} onClick={installSkill}>
          {installing ? "Installing…" : "Install / update Hermes skill"}
        </button>
      </div>
      <p className="subtitle" style={{ marginTop: 12 }}>
        Manual setup: copy <span className="mono">hermes-skill/</span> to{" "}
        <span className="mono">~/.hermes/skills/llama-switcher/</span>, then add
        these values to <span className="mono">~/.hermes/.env</span>:
      </p>
      <pre className="code-block">{`LLAMA_SWITCHER_BASE_URL=${info.baseUrl}\nLLAMA_SWITCHER_API_TOKEN=${info.token}`}</pre>
      <p className="subtitle">
        No registration or npm build is needed. Start a new Hermes session or
        run <span className="mono">/reset</span> after installing.
      </p>
    </div>
  );
}
