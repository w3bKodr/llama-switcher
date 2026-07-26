import type { Status } from "../types";

function formatVram(mib: number | null) {
  if (mib == null) return "—";
  return `${(mib / 1024).toFixed(mib >= 10_240 ? 0 : 1)} GB`;
}

function stateMeta(status: Status) {
  if (!status.serverReachable) {
    return { label: "Down", badge: "red", tone: "down" };
  }
  if (status.running && !status.healthy) {
    return { label: "Starting", badge: "yellow", tone: "busy" };
  }
  if (status.usageState === "busy") {
    return { label: "In use", badge: "yellow", tone: "busy" };
  }
  if (status.usageState === "free") {
    return { label: "Free", badge: "green", tone: "healthy" };
  }
  return { label: "Ready", badge: "green", tone: "healthy" };
}

function healthLabel(status: Status) {
  if (!status.serverReachable) return "Offline";
  if (status.running && !status.healthy) return "Loading";
  return "Healthy";
}

function usageLabel(status: Status) {
  if (!status.serverReachable) return "Unavailable";
  if (status.running && !status.healthy) return "Waiting for server";
  if (status.usageState === "busy") return "In use";
  if (status.usageState === "free") return "Free";
  return "Unknown";
}

export function StatusCard({ status }: { status: Status | null }) {
  if (!status) {
    return <div className="card">Loading status...</div>;
  }

  const state = stateMeta(status);
  const title = status.running
    ? status.model
    : status.serverReachable
      ? "Externally started server"
      : "No server running";

  return (
    <div className={`card status-card ${state.tone}`}>
      <div className="status-hero">
        <div>
          <div className="status-kicker">Current model</div>
          <strong className="status-title">{title ?? "Unknown profile"}</strong>
          {status.feature && <span className="status-feature">{status.feature}</span>}
          {!status.running && status.serverReachable && (
            <p className="subtitle status-note">
              Llama Switcher can stop this server directly or replace it the next time you switch profiles.
            </p>
          )}
        </div>
        <div className="status-pills">
          <span className={`badge ${state.badge}`}>{state.label}</span>
        </div>
      </div>

      <div className="status-summary">
        <div className="summary-chip">
          <span className="summary-label">Avg Tk/s</span>
          <span className="summary-value">
            {status.avgTokensPerSecond != null
              ? status.avgTokensPerSecond.toFixed(1)
              : "—"}
          </span>
        </div>
        <div className="summary-chip">
          <span className="summary-label">Health</span>
          <span className="summary-value">{healthLabel(status)}</span>
        </div>
        <div className="summary-chip">
          <span className="summary-label">Usage</span>
          <span className="summary-value">{usageLabel(status)}</span>
        </div>
      </div>

      <section className="vram-card" aria-label="VRAM usage">
        <div className="vram-heading">
          <div>
            <span className="status-kicker">GPU memory</span>
            <strong>VRAM</strong>
          </div>
          <span className="vram-total">
            {status.vram.totalMib == null
              ? "NVIDIA GPU unavailable"
              : `${formatVram(status.vram.freeMib)} available`}
          </span>
        </div>
        {status.vram.totalMib != null ? (
          <>
            <div className="vram-bar" aria-label={`${formatVram(status.vram.usedMib)} of ${formatVram(status.vram.totalMib)} in use`}>
              <span style={{ width: `${Math.min(100, ((status.vram.usedMib ?? 0) / status.vram.totalMib) * 100)}%` }} />
            </div>
            <div className="vram-details">
              <span><b>{formatVram(status.vram.usedMib)}</b> used of {formatVram(status.vram.totalMib)}</span>
              <span><b>{formatVram(status.vram.modelMib)}</b> server allocation</span>
            </div>
            <details className="vram-processes">
                <summary>VRAM by process <span>{status.vram.processes.length || "Unavailable"}</span></summary>
                <div className="vram-process-list">
                  {status.vram.processes.length > 0 ? status.vram.processes.map((process) => (
                    <div className="vram-process" key={process.pid}>
                      <span title={process.name}>{process.name}</span>
                      <small>PID {process.pid}</small>
                      <b>{formatVram(process.usedMib)}</b>
                    </div>
                  )) : (
                    <p className="vram-process-unavailable">
                      No active GPU processes were reported by Windows.
                    </p>
                  )}
                </div>
                {status.vram.processes.length > 0 && (
                  <p className="vram-process-note">
                    Stale and overlapping Windows GPU allocations are filtered to match physical VRAM.
                  </p>
                )}
            </details>
          </>
        ) : (
          <p className="vram-unavailable">Install or update the NVIDIA driver to display live GPU memory.</p>
        )}
      </section>

      <div className="status-details">
        <div className="status-detail">
          <span>Server</span>
          <b>127.0.0.1:{status.serverPort}</b>
        </div>
        <div className="status-detail">
          <span>Process</span>
          <b>{status.pid ? `PID ${status.pid}` : "—"}</b>
        </div>
        <div className="status-detail">
          <span>Started</span>
          <b>{status.startedAt ?? "—"}</b>
        </div>
        <div className="status-detail" title={status.scriptPath ?? undefined}>
          <span>Launch script</span>
          <b className="detail-script">{status.scriptPath?.split(/[\\/]/).pop() ?? "—"}</b>
        </div>
      </div>
    </div>
  );
}
