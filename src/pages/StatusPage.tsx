import { useEffect, useState, useCallback } from "react";
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
  const [portLive, setPortLive] = useState<boolean>(status?.serverReachable ?? false);

  // Light probe (no takeover side-effect) to decide if Stop should be enabled.
  const checkPort = useCallback(async () => {
    try {
      setPortLive(await api.isServerReachable());
    } catch {
      setPortLive(false);
    }
  }, []);

  // Probe every 3s so the Stop button reflects reality even for external servers.
  useEffect(() => {
    checkPort();
    const id = setInterval(checkPort, 3000);
    return () => clearInterval(id);
  }, [checkPort]);

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
  const serverPresent = running || portLive;

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
