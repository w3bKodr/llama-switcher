import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Nav, type Page } from "./components/Nav";
import { StatusPage } from "./pages/StatusPage";
import { DetectedScriptsPage } from "./pages/DetectedScriptsPage";
import { SettingsPage } from "./pages/SettingsPage";
import { LogsPage } from "./pages/LogsPage";
import { AgentControlPage } from "./pages/AgentControlPage";
import { api } from "./api";
import type { Status } from "./types";

export interface Toast {
  message: string;
  error: boolean;
}

export default function App() {
  const [page, setPage] = useState<Page>("status");
  const [status, setStatus] = useState<Status | null>(null);
  const [toast, setToast] = useState<Toast | null>(null);

  const refreshStatus = useCallback(async () => {
    try {
      setStatus(await api.getStatus());
    } catch (e) {
      console.error(e);
    }
  }, []);

  const showToast = useCallback((message: string, error = false) => {
    setToast({ message, error });
    setTimeout(() => setToast(null), error ? 6000 : 3000);
  }, []);

  useEffect(() => {
    refreshStatus();
    // The backend emits "status-changed" whenever the process state changes
    // (start/stop/switch/health). Re-fetch via get_status so the live health
    // probe (server reachability) is included. Event-driven, no polling.
    const unlistenStatus = listen<Status>("status-changed", () => {
      refreshStatus();
    });
    const unlistenNav = listen<string>("navigate", (e) => {
      setPage(e.payload as Page);
    });
    return () => {
      unlistenStatus.then((f) => f());
      unlistenNav.then((f) => f());
    };
  }, [refreshStatus]);

  return (
    <div className="app">
      <Nav page={page} setPage={setPage} running={status?.running ?? false} />
      <div className="content">
        {page === "status" && (
          <StatusPage status={status} onAction={refreshStatus} showToast={showToast} />
        )}
        {page === "scripts" && (
          <DetectedScriptsPage status={status} onAction={refreshStatus} showToast={showToast} />
        )}
        {page === "settings" && <SettingsPage showToast={showToast} />}
        {page === "logs" && <LogsPage showToast={showToast} />}
        {page === "agent" && <AgentControlPage showToast={showToast} />}
      </div>
      {toast && (
        <div className={`toast ${toast.error ? "error" : ""}`}>{toast.message}</div>
      )}
    </div>
  );
}
