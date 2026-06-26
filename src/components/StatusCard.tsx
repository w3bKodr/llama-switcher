import type { Status } from "../types";

function Row({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="row">
      <span className="label">{label}</span>
      <span className="value">{value ?? "-"}</span>
    </div>
  );
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
    ? status.alias
    : status.serverReachable
      ? "Externally started server"
      : "No server running";

  return (
    <div className={`card status-card ${state.tone}`}>
      <div className="status-hero">
        <div>
          <div className="status-kicker">Current server</div>
          <strong className="status-title">{title ?? "Unknown profile"}</strong>
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
          <span className="summary-label">State</span>
          <span className="summary-value">{state.label}</span>
        </div>
        <div className="summary-chip">
          <span className="summary-label">Model</span>
          <span className="summary-value">{status.model ?? "-"}</span>
        </div>
        <div className="summary-chip">
          <span className="summary-label">Feature</span>
          <span className="summary-value">{status.feature ?? "-"}</span>
        </div>
      </div>

      <Row label="Model" value={status.model} />
      <Row label="Feature" value={status.feature} />
      <Row label="Health" value={healthLabel(status)} />
      <Row label="Usage" value={usageLabel(status)} />
      <Row label="PID" value={status.pid} />
      <Row label="Server URL" value={`http://127.0.0.1:${status.serverPort}`} />
      <Row label="Health URL" value={status.healthUrl} />
      <Row label="Script path" value={<span className="mono">{status.scriptPath}</span>} />
      <Row label="Started" value={status.startedAt} />
    </div>
  );
}
