import { useEffect, useState } from "react";
import { api } from "../api";
import { LogViewer } from "../components/LogViewer";
import type { LogEntry } from "../types";

export function LogsPage({
  showToast,
}: {
  showToast: (m: string, e?: boolean) => void;
}) {
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [text, setText] = useState("");

  async function loadList() {
    try {
      const list = await api.listLogs();
      setLogs(list);
      if (list.length > 0) {
        const first = selected ?? list[0].path;
        setSelected(first);
        setText(await api.readLog(first));
      } else {
        setText("");
      }
    } catch (e) {
      showToast(`Failed to load logs: ${String(e)}`, true);
    }
  }

  useEffect(() => {
    loadList();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function open(path: string) {
    setSelected(path);
    try {
      setText(await api.readLog(path));
    } catch (e) {
      showToast(`Failed to read log: ${String(e)}`, true);
    }
  }

  async function clearOld() {
    try {
      const removed = await api.clearOldLogs();
      showToast(`Cleared ${removed} old log file(s).`);
      loadList();
    } catch (e) {
      showToast(`Clear failed: ${String(e)}`, true);
    }
  }

  return (
    <div>
      <div className="spread">
        <h1>Logs</h1>
        <div className="inline">
          <button className="btn" onClick={loadList}>
            Refresh
          </button>
          <button className="btn" onClick={() => api.openLogsFolder()}>
            Open logs folder
          </button>
          <button className="btn danger" onClick={clearOld}>
            Clear old logs
          </button>
        </div>
      </div>
      <p className="subtitle">One log file per server run.</p>

      <div className="field" style={{ maxWidth: 480 }}>
        <label>Run log</label>
        <select
          value={selected ?? ""}
          onChange={(e) => open(e.target.value)}
        >
          {logs.length === 0 && <option value="">No logs yet</option>}
          {logs.map((l) => (
            <option key={l.path} value={l.path}>
              {l.filename} — {l.modifiedAt}
            </option>
          ))}
        </select>
      </div>

      <LogViewer text={text} />
    </div>
  );
}
