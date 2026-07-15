import type { Status } from "../types";

export type Page = "status" | "scripts" | "benchmark" | "settings" | "logs" | "agent";

const ITEMS: { key: Page; label: string }[] = [
  { key: "status", label: "Status" },
  { key: "scripts", label: "Detected Scripts" },
  { key: "benchmark", label: "Benchmark" },
  { key: "settings", label: "Settings" },
  { key: "logs", label: "Logs" },
  { key: "agent", label: "Agent Control" },
];

export function Nav({
  page,
  setPage,
  status,
}: {
  page: Page;
  setPage: (p: Page) => void;
  status: Status | null;
}) {
  const dotClass = !status || (!status.running && !status.serverReachable)
    ? "down"
    : status.usageState === "busy" || (status.running && !status.healthy)
      ? "busy"
      : "healthy";

  return (
    <nav className="nav">
      <div className="nav-brand">
        <span className={`dot ${dotClass}`} />
        Llama Switcher
      </div>
      {ITEMS.map((it) => (
        <button
          key={it.key}
          className={`nav-item ${page === it.key ? "active" : ""}`}
          onClick={() => setPage(it.key)}
        >
          {it.label}
        </button>
      ))}
    </nav>
  );
}
