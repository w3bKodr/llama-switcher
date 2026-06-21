import { useEffect, useState } from "react";
import { api } from "../api";
import { ProfileTable } from "../components/ProfileTable";
import type { ScanResult, Status } from "../types";

export function DetectedScriptsPage({
  status,
  onAction,
  showToast,
}: {
  status: Status | null;
  onAction: () => void;
  showToast: (m: string, e?: boolean) => void;
}) {
  const [scan, setScan] = useState<ScanResult | null>(null);
  const [filter, setFilter] = useState("");
  const [showIgnored, setShowIgnored] = useState(false);
  const [busy, setBusy] = useState(false);

  async function load() {
    try {
      setScan(await api.getScanResult());
    } catch (e) {
      showToast(`Failed to load scan: ${String(e)}`, true);
    }
  }

  useEffect(() => {
    load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function rescan() {
    setBusy(true);
    try {
      const result = await api.rescanScripts();
      setScan(result);
      showToast(`Rescanned: ${result.profiles.length} profiles found.`);
    } catch (e) {
      showToast(`Rescan failed: ${String(e)}`, true);
    } finally {
      setBusy(false);
    }
  }

  const profiles = (scan?.profiles ?? []).filter((p) =>
    filter.trim() === ""
      ? true
      : p.alias.toLowerCase().includes(filter.toLowerCase())
  );

  return (
    <div>
      <div className="spread">
        <h1>Detected Scripts</h1>
        <button className="btn" disabled={busy} onClick={rescan}>
          {busy ? "Rescanning…" : "Rescan folder"}
        </button>
      </div>
      <p className="subtitle">
        Profiles auto-generated from <span className="mono">start - {"{model}"} - {"{feature}"}</span> files.
        {scan && <> &nbsp;Scanned at {scan.scannedAt}.</>}
      </p>

      <div className="field" style={{ maxWidth: 320 }}>
        <input
          type="text"
          placeholder="Filter by alias…"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
      </div>

      <ProfileTable
        profiles={profiles}
        status={status}
        onAction={onAction}
        showToast={showToast}
      />

      {scan && scan.ignoredFiles.length > 0 && (
        <div className="card" style={{ marginTop: 18 }}>
          <div
            className="collapsible-header"
            onClick={() => setShowIgnored((v) => !v)}
          >
            <span>{showIgnored ? "▾" : "▸"}</span>
            <strong>Ignored files ({scan.ignoredFiles.length})</strong>
          </div>
          {showIgnored && (
            <table style={{ marginTop: 10 }}>
              <thead>
                <tr>
                  <th>File</th>
                  <th>Reason</th>
                </tr>
              </thead>
              <tbody>
                {scan.ignoredFiles.map((f) => (
                  <tr key={f.filename}>
                    <td className="mono">{f.filename}</td>
                    <td className="label">{f.reason}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      )}
    </div>
  );
}
