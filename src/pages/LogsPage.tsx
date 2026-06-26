import { useEffect, useRef, useState } from "react";
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
  const [offset, setOffset] = useState(0);
  const latestRef = useRef<string | null>(null);
  const offsetRef = useRef(0);

  useEffect(() => {
    offsetRef.current = offset;
  }, [offset]);

  async function open(path: string, keepPosition = false) {
    setSelected(path);
    try {
      const full = await api.readLog(path);
      setText(full);
      setOffset(new Blob([full]).size);
      if (!keepPosition) {
        latestRef.current = path;
      }
    } catch (e) {
      showToast(`Failed to read log: ${String(e)}`, true);
    }
  }

  async function loadList(preferred?: string | null) {
    try {
      const list = await api.listLogs();
      setLogs(list);

      if (list.length === 0) {
        setSelected(null);
        setText("");
        setOffset(0);
        latestRef.current = null;
        return;
      }

      const newest = list[0].path;
      const previousLatest = latestRef.current;
      latestRef.current = newest;

      const next =
        preferred && list.some((entry) => entry.path === preferred)
          ? preferred
          : selected && list.some((entry) => entry.path === selected)
            ? selected
            : newest;

      const shouldJumpToNewest =
        !selected || selected === previousLatest || !list.some((entry) => entry.path === selected);

      if (shouldJumpToNewest && next !== newest) {
        await open(newest);
        return;
      }

      if (next !== selected) {
        await open(next, true);
      }
    } catch (e) {
      showToast(`Failed to load logs: ${String(e)}`, true);
    }
  }

  useEffect(() => {
    void loadList();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    let cancelled = false;

    const tick = async () => {
      if (cancelled || document.hidden || !selected) {
        return;
      }
      try {
        const update = await api.readLogUpdate(selected, offsetRef.current);
        if (cancelled) {
          return;
        }
        if (update.truncated) {
          const full = await api.readLog(selected);
          if (!cancelled) {
            setText(full);
            setOffset(new Blob([full]).size);
          }
          return;
        }
        if (update.text) {
          setText((current) => current + update.text);
        }
        setOffset(update.nextOffset);
      } catch {
        // ignore transient read errors while a new log file is being created
      }
    };

    const refreshLogs = async () => {
      if (!cancelled && !document.hidden) {
        await loadList(selected);
      }
    };

    const streamId = window.setInterval(() => {
      void tick();
    }, 1200);
    const listId = window.setInterval(() => {
      void refreshLogs();
    }, 4000);
    const onVisibility = () => {
      if (!document.hidden) {
        void refreshLogs();
        void tick();
      }
    };

    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      cancelled = true;
      window.clearInterval(streamId);
      window.clearInterval(listId);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [selected]);

  async function clearOld() {
    try {
      const removed = await api.clearOldLogs();
      showToast(`Cleared ${removed} old log file(s).`);
      await loadList(selected);
    } catch (e) {
      showToast(`Clear failed: ${String(e)}`, true);
    }
  }

  const currentIsLatest = !!selected && logs[0]?.path === selected;

  return (
    <div>
      <div className="spread">
        <div>
          <h1>Logs</h1>
          <p className="subtitle">Live run output with incremental updates. No manual refresh needed.</p>
        </div>
        <div className="inline">
          <button className="btn" onClick={() => void loadList(selected)}>
            Refresh list
          </button>
          <button className="btn" onClick={() => api.openLogsFolder()}>
            Open logs folder
          </button>
          <button className="btn danger" onClick={clearOld}>
            Clear old logs
          </button>
        </div>
      </div>

      <div className="field" style={{ maxWidth: 560 }}>
        <label>Run log</label>
        <div className="inline">
          <select
            value={selected ?? ""}
            onChange={(e) => {
              void open(e.target.value);
            }}
          >
            {logs.length === 0 && <option value="">No logs yet</option>}
            {logs.map((log) => (
              <option key={log.path} value={log.path}>
                {log.filename} — {log.modifiedAt}
              </option>
            ))}
          </select>
          <span className={`badge ${currentIsLatest ? "green" : "yellow"}`}>
            {currentIsLatest ? "Live" : "Historical"}
          </span>
        </div>
      </div>

      <LogViewer key={selected ?? "no-log"} text={text} follow={currentIsLatest} />
    </div>
  );
}
