export type Page = "status" | "scripts" | "settings" | "logs" | "agent";

const ITEMS: { key: Page; label: string }[] = [
  { key: "status", label: "Status" },
  { key: "scripts", label: "Detected Scripts" },
  { key: "settings", label: "Settings" },
  { key: "logs", label: "Logs" },
  { key: "agent", label: "Agent Control" },
];

export function Nav({
  page,
  setPage,
  running,
}: {
  page: Page;
  setPage: (p: Page) => void;
  running: boolean;
}) {
  return (
    <nav className="nav">
      <div className="nav-brand">
        <span className={`dot ${running ? "running" : ""}`} />
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
