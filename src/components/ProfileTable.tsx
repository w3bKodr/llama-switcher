import { useState } from "react";
import { api } from "../api";
import type { Profile, Status } from "../types";

export function ProfileTable({
  profiles,
  status,
  onAction,
  showToast,
}: {
  profiles: Profile[];
  status: Status | null;
  onAction: () => void;
  showToast: (m: string, e?: boolean) => void;
}) {
  const [busy, setBusy] = useState<string | null>(null);

  async function run(id: string, label: string, fn: () => Promise<unknown>) {
    setBusy(id);
    try {
      await fn();
      showToast(`${label} succeeded.`);
      onAction();
    } catch (e) {
      showToast(`${label} failed: ${String(e)}`, true);
    } finally {
      setBusy(null);
    }
  }

  // Group by pretty model, preserving discovery order.
  const groups: { model: string; items: Profile[] }[] = [];
  for (const p of profiles) {
    let g = groups.find((x) => x.model === p.prettyModel);
    if (!g) {
      g = { model: p.prettyModel, items: [] };
      groups.push(g);
    }
    g.items.push(p);
  }

  if (profiles.length === 0) {
    return <p className="subtitle">No profiles detected. Check your scripts folder and rescan.</p>;
  }

  return (
    <div>
      {groups.map((g) => (
        <div key={g.model}>
          <div className="model-group-title">{g.model}</div>
          <table>
            <thead>
              <tr>
                <th>Alias</th>
                <th>Feature</th>
                <th>File</th>
                <th>Ext</th>
                <th>Actions</th>
              </tr>
            </thead>
            <tbody>
              {g.items.map((p) => {
                const isCurrent = status?.running && status.currentProfileId === p.id;
                const filename = p.scriptPath.split(/[\\/]/).pop() ?? p.scriptPath;
                return (
                  <tr key={p.id}>
                    <td>
                      {p.alias}{" "}
                      {isCurrent && <span className="badge green">running</span>}
                    </td>
                    <td>{p.prettyFeature}</td>
                    <td className="path" title={p.scriptPath}>
                      {filename}
                    </td>
                    <td>{p.extension}</td>
                    <td>
                      <div className="inline">
                        <button
                          className="btn small primary"
                          disabled={busy !== null}
                          onClick={() =>
                            run(p.id, `Switch to ${p.alias}`, () => api.switchProfile(p.id))
                          }
                        >
                          {isCurrent ? "Restart" : "Start / Switch"}
                        </button>
                      </div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      ))}
    </div>
  );
}
