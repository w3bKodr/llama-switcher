import type { Status } from "../types";

function Row({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="row">
      <span className="label">{label}</span>
      <span className="value">{value ?? "—"}</span>
    </div>
  );
}

export function StatusCard({ status }: { status: Status | null }) {
  if (!status) {
    return <div className="card">Loading status…</div>;
  }

  return (
    <div className="card">
      <div className="spread" style={{ marginBottom: 8 }}>
        <strong style={{ fontSize: 16 }}>
          {status.running
            ? status.alias
            : status.serverReachable
              ? "Externally started server"
              : "No server running"}
        </strong>
        {status.running ? (
          status.healthy ? (
            <span className="badge green">Healthy</span>
          ) : (
            <span className="badge yellow">Starting / Unhealthy</span>
          )
        ) : status.serverReachable ? (
          <span className="badge green">Reachable</span>
        ) : (
          <span className="badge red">Stopped</span>
        )}
      </div>
      {!status.running && status.serverReachable && (
        <p className="subtitle" style={{ margin: "0 0 8px" }}>
          A llama.cpp server is answering on port {status.serverPort}. Llama
          Switcher can stop it directly or replace it when you Start / Switch to
          another profile.
        </p>
      )}
      <Row label="Model" value={status.model} />
      <Row label="Feature" value={status.feature} />
      <Row label="PID" value={status.pid} />
      <Row label="Server URL" value={`http://127.0.0.1:${status.serverPort}`} />
      <Row label="Health URL" value={status.healthUrl} />
      <Row label="Script path" value={<span className="mono">{status.scriptPath}</span>} />
      <Row label="Started" value={status.startedAt} />
    </div>
  );
}
