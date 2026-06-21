import { useState } from "react";
import { api } from "../api";
import { StatusCard } from "../components/StatusCard";
import type { Status } from "../types";

export function StatusPage({
  status,
  onAction,
  showToast,
}: {
  status: Status | null;
  onAction: () => void;
  showToast: (m: string, e?: boolean) => void;
}) {
  const [busy, setBusy] = useState(false);

  async function run(label: string, fn: () => Promise<unknown>) {
    setBusy(true);
    try {
      await fn();
      showToast(`${label} succeeded.`);
      onAction();
    } catch (e) {
      showToast(`${label} failed: ${String(e)}`, true);
    } finally {
      setBusy(false);
    }
  }

  const running = status?.running ?? false;
  const serverPresent = running || (status?.serverReachable ?? false);

  return (
    <div>
      <h1>Status</h1>
      <p className="subtitle">Current llama.cpp server managed by Llama Switcher.</p>

      <StatusCard status={status} />

      <div className="btn-row">
        <button
          className="btn danger"
          disabled={busy || !serverPresent}
          onClick={() => run("Stop", api.stopServer)}
        >
          Stop
        </button>
        <button
          className="btn"
          disabled={busy || !running}
          onClick={() => run("Restart", api.restartServer)}
        >
          Restart
        </button>
        <button className="btn" disabled={busy} onClick={() => run("Rescan", api.rescanScripts)}>
          Rescan
        </button>
        <button className="btn" disabled={busy} onClick={() => run("Open scripts folder", api.openScriptsFolder)}>
          Open scripts folder
        </button>
        <button className="btn" disabled={busy} onClick={() => run("Open logs folder", api.openLogsFolder)}>
          Open logs folder
        </button>
      </div>
    </div>
  );
}
