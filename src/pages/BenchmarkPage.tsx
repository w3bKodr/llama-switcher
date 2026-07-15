import { useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "../api";
import type { BenchmarkPrompt, BenchmarkProgress, Profile } from "../types";

// Mirror of the Rust `sanitize_alias`: whitespace and illegal chars -> '-',
// collapse repeats. Used only to build the "Open folder" path for each model.
function sanitizeAlias(alias: string): string {
  return alias
    .split("")
    .map((c) => (/\s/.test(c) || "<>:\"/\\|?*".includes(c) ? "-" : c))
    .join("")
    .split("-")
    .filter((p) => p.length > 0)
    .join("-");
}

type CellState = "pending" | "running" | "done" | "error";
type ModelState = "pending" | "switching" | "ready" | "error";

function cellKey(profileId: string, promptId: string) {
  return `${profileId}::${promptId}`;
}

const CELL_ICON: Record<CellState, string> = {
  pending: "·",
  running: "…",
  done: "✓",
  error: "✕",
};

function formatHMS(seconds: number): string {
  const total = Math.max(0, Math.floor(seconds));
  const hh = Math.floor(total / 3600);
  const mm = Math.floor((total % 3600) / 60);
  const ss = total % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(hh)}:${pad(mm)}:${pad(ss)}`;
}

export function BenchmarkPage({
  showToast,
  active,
}: {
  showToast: (m: string, e?: boolean) => void;
  active: boolean;
}) {
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [selectedIds, setSelectedIds] = useState<string[]>([]);
  const [prompts, setPrompts] = useState<BenchmarkPrompt[]>([]);
  const [outputDir, setOutputDir] = useState("");
  const [timeoutSeconds, setTimeoutSeconds] = useState(600);

  const [running, setRunning] = useState(false);
  const [cells, setCells] = useState<Record<string, CellState>>({});
  const [durations, setDurations] = useState<Record<string, number>>({});
  const [tps, setTps] = useState<Record<string, number>>({});
  const [modelStates, setModelStates] = useState<Record<string, ModelState>>({});
  const [errors, setErrors] = useState<string[]>([]);
  const promptSeq = useRef(3);

  // Load persisted config + detected profiles.
  useEffect(() => {
    (async () => {
      try {
        const [cfg, profs, isRunning] = await Promise.all([
          api.getBenchmarkConfig(),
          api.getDetectedProfiles(),
          api.isBenchmarkRunning(),
        ]);
        setProfiles(profs);
        setPrompts(cfg.prompts);
        setOutputDir(cfg.outputDir);
        setTimeoutSeconds(cfg.timeoutSeconds);
        setRunning(isRunning);
        const valid = cfg.profileIds.filter((id) => profs.some((p) => p.id === id));
        setSelectedIds(valid.length > 0 ? valid : profs.slice(0, 2).map((p) => p.id));
      } catch (e) {
        showToast(`Failed to load benchmark config: ${String(e)}`, true);
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // The page stays mounted across tab switches (so progress/timers persist);
  // refresh just the model list each time the tab is reopened, without touching
  // progress state or in-progress prompt edits.
  useEffect(() => {
    if (!active) return;
    api.getDetectedProfiles().then(setProfiles).catch(() => {});
  }, [active]);

  // Live progress from the backend runner.
  useEffect(() => {
    const un = listen<BenchmarkProgress>("benchmark-progress", (e) => {
      const p = e.payload;
      if (p.kind === "run") {
        if (p.status === "running") {
          setRunning(true);
          setCells({});
          setDurations({});
          setTps({});
          setModelStates({});
          setErrors([]);
        } else if (p.status === "finished" || p.status === "cancelled") {
          setRunning(false);
          showToast(`Benchmark ${p.status}.`);
        }
      } else if (p.kind === "model" && p.profileId) {
        const ms: ModelState =
          p.status === "switching"
            ? "switching"
            : p.status === "error"
              ? "error"
              : "ready";
        setModelStates((m) => ({ ...m, [p.profileId!]: ms }));
        if (p.status === "error" && p.message) {
          setErrors((es) => [...es, `${p.alias ?? p.profileId}: ${p.message}`]);
        }
      } else if (p.kind === "prompt" && p.profileId && p.promptId) {
        const cs: CellState =
          p.status === "running" ? "running" : p.status === "done" ? "done" : "error";
        const key = cellKey(p.profileId!, p.promptId!);
        setCells((c) => ({ ...c, [key]: cs }));
        if (p.status === "done" && p.durationSeconds != null) {
          setDurations((d) => ({ ...d, [key]: p.durationSeconds! }));
        }
        if (p.status === "done" && p.tokensPerSecond != null) {
          setTps((t) => ({ ...t, [key]: p.tokensPerSecond! }));
        }
        if (p.status === "error" && p.message) {
          setErrors((es) => [...es, `${p.alias ?? p.profileId} / ${p.promptId}: ${p.message}`]);
        }
      }
    });
    return () => {
      un.then((f) => f());
    };
  }, [showToast]);

  const selectedProfiles = useMemo(
    () =>
      selectedIds
        .map((id) => profiles.find((p) => p.id === id))
        .filter((p): p is Profile => !!p),
    [selectedIds, profiles]
  );

  function toggleModel(id: string) {
    setSelectedIds((ids) =>
      ids.includes(id) ? ids.filter((x) => x !== id) : [...ids, id]
    );
  }

  function updatePrompt(i: number, patch: Partial<BenchmarkPrompt>) {
    setPrompts((ps) => ps.map((p, idx) => (idx === i ? { ...p, ...patch } : p)));
  }

  function addPrompt() {
    const id = `prompt${promptSeq.current++}`;
    setPrompts((ps) => [...ps, { id, title: "New prompt", text: "" }]);
  }

  function removePrompt(i: number) {
    setPrompts((ps) => ps.filter((_, idx) => idx !== i));
  }

  async function browse() {
    try {
      const dir = await api.browseFolder();
      if (dir) setOutputDir(dir);
    } catch (e) {
      showToast(`Browse failed: ${String(e)}`, true);
    }
  }

  async function run() {
    if (selectedIds.length === 0) return showToast("Select at least one model.", true);
    if (prompts.length === 0) return showToast("Add at least one prompt.", true);
    if (!outputDir.trim()) return showToast("Choose an output folder.", true);
    try {
      await api.runBenchmark({
        profileIds: selectedIds,
        prompts,
        outputDir,
        timeoutSeconds,
      });
      showToast("Benchmark started.");
    } catch (e) {
      showToast(`Could not start: ${String(e)}`, true);
    }
  }

  async function cancel() {
    try {
      await api.cancelBenchmark();
      showToast("Cancelling after the current step…");
    } catch (e) {
      showToast(String(e), true);
    }
  }

  return (
    <div>
      <div className="spread">
        <h1>Benchmark</h1>
        <div className="inline">
          {running ? (
            <button className="btn danger" onClick={cancel}>
              Cancel
            </button>
          ) : (
            <button className="btn primary" onClick={run}>
              Run benchmark
            </button>
          )}
        </div>
      </div>
      <p className="subtitle">
        Runs every selected model through every prompt in order (switch model → all
        prompts → next model). Output is saved per model and prompt.
      </p>

      {/* Models */}
      <div className="card">
        <h2 style={{ marginTop: 0 }}>Models ({selectedIds.length} selected)</h2>
        {profiles.length === 0 ? (
          <p className="subtitle">No detected models. Add scripts and rescan.</p>
        ) : (
          <div className="bench-model-list">
            {profiles.map((p) => (
              <label key={p.id} className="bench-model">
                <input
                  type="checkbox"
                  checked={selectedIds.includes(p.id)}
                  onChange={() => toggleModel(p.id)}
                  disabled={running}
                />
                {p.alias}
              </label>
            ))}
          </div>
        )}
      </div>

      {/* Prompts */}
      <div className="card">
        <div className="spread">
          <h2 style={{ marginTop: 0 }}>Prompts ({prompts.length})</h2>
          <button className="btn small" onClick={addPrompt} disabled={running}>
            + Add prompt
          </button>
        </div>
        {prompts.map((p, i) => (
          <div key={p.id} className="bench-prompt">
            <div className="inline" style={{ marginBottom: 6 }}>
              <input
                type="text"
                value={p.title}
                onChange={(e) => updatePrompt(i, { title: e.target.value })}
                disabled={running}
                placeholder={`Prompt ${i + 1} title`}
              />
              <button
                className="btn small danger"
                onClick={() => removePrompt(i)}
                disabled={running || prompts.length <= 1}
              >
                Remove
              </button>
            </div>
            <textarea
              className="bench-textarea"
              value={p.text}
              onChange={(e) => updatePrompt(i, { text: e.target.value })}
              disabled={running}
              rows={4}
            />
          </div>
        ))}
      </div>

      {/* Output + timeout */}
      <div className="card">
        <h2 style={{ marginTop: 0 }}>Output</h2>
        <div className="field">
          <label>Output folder</label>
          <div className="inline">
            <input
              type="text"
              value={outputDir}
              onChange={(e) => setOutputDir(e.target.value)}
              disabled={running}
              placeholder="Choose where results are written"
            />
            <button className="btn" onClick={browse} disabled={running}>
              Browse…
            </button>
          </div>
          <span className="hint">
            Creates <span className="mono">&lt;folder&gt;/&lt;Model-Alias&gt;/promptN/</span> with
            response.md, extracted code (index.html / image.svg), and meta.json.
          </span>
        </div>
        <div className="field" style={{ maxWidth: 240 }}>
          <label>Per-prompt timeout (seconds)</label>
          <input
            type="number"
            value={timeoutSeconds}
            onChange={(e) => setTimeoutSeconds(Number(e.target.value))}
            disabled={running}
          />
        </div>
      </div>

      {/* Progress grid */}
      {selectedProfiles.length > 0 && (
        <div className="card">
          <h2 style={{ marginTop: 0 }}>Progress</h2>
          <div className="bench-grid-wrap">
            <table className="bench-grid">
              <thead>
                <tr>
                  <th>Model</th>
                  {prompts.map((p, i) => (
                    <th key={p.id} title={p.title}>
                      #{i + 1}
                    </th>
                  ))}
                  <th></th>
                </tr>
              </thead>
              <tbody>
                {selectedProfiles.map((prof) => {
                  const ms = modelStates[prof.id] ?? "pending";
                  return (
                    <tr key={prof.id}>
                      <td>
                        {prof.alias}{" "}
                        {ms === "switching" && <span className="badge yellow">switching…</span>}
                        {ms === "error" && <span className="badge red">error</span>}
                      </td>
                      {prompts.map((p) => {
                        const key = cellKey(prof.id, p.id);
                        const cs = cells[key] ?? "pending";
                        const dur = durations[key];
                        const speed = tps[key];
                        return (
                          <td key={p.id} className={`bench-cell ${cs}`} title={cs}>
                            {cs === "done" ? (
                              <div className="bench-cell-done">
                                <span>{dur != null ? formatHMS(dur) : "✓"}</span>
                                {speed != null && (
                                  <span className="bench-tps">{speed.toFixed(1)} tk/s</span>
                                )}
                              </div>
                            ) : (
                              CELL_ICON[cs]
                            )}
                          </td>
                        );
                      })}
                      <td>
                        <button
                          className="btn small"
                          onClick={() =>
                            api.openPath(`${outputDir}\\${sanitizeAlias(prof.alias)}`)
                          }
                          disabled={!outputDir}
                        >
                          Open folder
                        </button>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
          {errors.length > 0 && (
            <div className="bench-errors">
              {errors.map((e, i) => (
                <div key={i} className="bench-error">
                  {e}
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
