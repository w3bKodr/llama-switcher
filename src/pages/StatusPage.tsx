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
  const [busyAction, setBusyAction] = useState<string | null>(null);

  async function run(label: string, fn: () => Promise<unknown>) {
    setBusyAction(label);
    try {
      await fn();
      showToast(`${label} succeeded.`);
      onAction();
    } catch (e) {
      showToast(`${label} failed: ${String(e)}`, true);
    } finally {
      setBusyAction(null);
    }
  }

  const running = status?.running ?? false;
  return (
    <div className="status-page">
      <h1>Status</h1>
      <p className="subtitle">Live server state, usage, and controls with automatic background refresh.</p>

      <StatusCard status={status} />

      <div className="btn-row status-actions">
        <button
          className="btn danger"
          disabled={busyAction !== null}
          onClick={() => run("Stop", api.stopServer)}
        >
          {busyAction === "Stop" ? "Stopping…" : "Stop"}
        </button>
        <button
          className="btn"
          disabled={busyAction !== null || !running}
          onClick={() => run("Restart", api.restartServer)}
        >
          {busyAction === "Restart" ? "Restarting…" : "Restart"}
        </button>
        <button className="btn" disabled={busyAction !== null} onClick={() => run("Rescan", api.rescanScripts)}>
          {busyAction === "Rescan" ? "Rescanning…" : "Rescan"}
        </button>
        <button className="btn" disabled={busyAction !== null} onClick={() => run("Open scripts folder", api.openScriptsFolder)}>
          Open scripts folder
        </button>
        <button className="btn" disabled={busyAction !== null} onClick={() => run("Open logs folder", api.openLogsFolder)}>
          Open logs folder
        </button>
      </div>
    </div>
  );
}
