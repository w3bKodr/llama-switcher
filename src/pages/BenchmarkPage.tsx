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
  const [modelStartTimeoutSeconds, setModelStartTimeoutSeconds] = useState(300);
  const [modelFilter, setModelFilter] = useState("");
  const [activePromptId, setActivePromptId] = useState<string | null>(null);

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
        setActivePromptId(cfg.prompts[0]?.id ?? null);
        setOutputDir(cfg.outputDir);
        setTimeoutSeconds(cfg.timeoutSeconds);
        setModelStartTimeoutSeconds(cfg.modelStartTimeoutSeconds ?? 300);
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
  const enabledPrompts = useMemo(
    () => prompts.filter((prompt) => prompt.enabled !== false),
    [prompts],
  );
  const visibleProfiles = useMemo(() => {
    const filter = modelFilter.trim().toLowerCase();
    if (!filter) return profiles;
    return profiles.filter((profile) =>
      `${profile.alias} ${profile.prettyModel} ${profile.prettyFeature}`.toLowerCase().includes(filter),
    );
  }, [modelFilter, profiles]);
  const modelGroups = useMemo(
    () => visibleProfiles.reduce<Array<{ model: string; profiles: Profile[] }>>((groups, profile) => {
      const existing = groups.find((group) => group.model === profile.prettyModel);
      if (existing) existing.profiles.push(profile);
      else groups.push({ model: profile.prettyModel, profiles: [profile] });
      return groups;
    }, []),
    [visibleProfiles],
  );
  const totalJobs = selectedProfiles.length * enabledPrompts.length;
  const finishedJobs = Object.values(cells).filter((state) => state === "done" || state === "error").length;
  const progressPercent = totalJobs > 0 ? Math.min(100, (finishedJobs / totalJobs) * 100) : 0;

  function toggleModel(id: string) {
    setSelectedIds((ids) =>
      ids.includes(id) ? ids.filter((x) => x !== id) : [...ids, id]
    );
  }

  function updatePrompt(i: number, patch: Partial<BenchmarkPrompt>) {
    setPrompts((ps) => ps.map((p, idx) => (idx === i ? { ...p, ...patch } : p)));
  }

  function addPrompt() {
    let id = `prompt${promptSeq.current++}`;
    while (prompts.some((prompt) => prompt.id === id)) {
      id = `prompt${promptSeq.current++}`;
    }
    setPrompts((ps) => [...ps, { id, title: "New prompt", text: "", enabled: true }]);
    setActivePromptId(id);
  }

  function removePrompt(i: number) {
    setPrompts((ps) => {
      const next = ps.filter((_, idx) => idx !== i);
      if (ps[i]?.id === activePromptId) {
        setActivePromptId(next[Math.min(i, next.length - 1)]?.id ?? null);
      }
      return next;
    });
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
    if (enabledPrompts.length === 0) return showToast("Select at least one prompt.", true);
    if (!outputDir.trim()) return showToast("Choose an output folder.", true);
    try {
      await api.runBenchmark({
        profileIds: selectedIds,
        prompts,
        outputDir,
        timeoutSeconds,
        modelStartTimeoutSeconds,
      });
      showToast("Benchmark started.");
    } catch (e) {
      showToast(`Could not start: ${String(e)}`, true);
    }
  }

  async function cancel() {
    try {
      await api.cancelBenchmark();
      setRunning(false);
      showToast("Benchmark cancelled.");
    } catch (e) {
      showToast(String(e), true);
    }
  }

  return (
    <div className="benchmark-page">
      <section className={`benchmark-hero ${running ? "is-running" : ""}`}>
        <div className="benchmark-hero-copy">
          <span className="status-kicker">Evaluation workspace</span>
          <h1>Benchmark</h1>
          <p>Compare every selected model against only the prompts you choose.</p>
        </div>
        <div className="benchmark-run-summary">
          <div><b>{selectedIds.length}</b><span>Models</span></div>
          <div><b>{enabledPrompts.length}</b><span>Prompts</span></div>
          <div><b>{totalJobs}</b><span>Total runs</span></div>
          <div><b>{formatHMS(modelStartTimeoutSeconds)}</b><span>Model startup</span></div>
        </div>
        <div className="benchmark-primary-action">
          {running ? (
            <button className="btn danger benchmark-run-button" onClick={() => void cancel()}><span>■</span> Cancel run</button>
          ) : (
            <button className="btn primary benchmark-run-button" onClick={() => void run()}><span>▶</span> Run benchmark</button>
          )}
          <small>{running ? `${finishedJobs} of ${totalJobs} completed` : `${totalJobs} queued evaluations`}</small>
        </div>
      </section>

      {/* Models */}
      <section className="benchmark-panel benchmark-model-panel">
        <div className="benchmark-panel-heading">
          <div>
            <span className="benchmark-step">01</span>
            <div><h2>Choose models</h2><p>Select the launch profiles to compare.</p></div>
          </div>
          <div className="benchmark-heading-actions">
            <button className="text-button" disabled={running || visibleProfiles.length === 0} onClick={() => setSelectedIds((ids) => Array.from(new Set([...ids, ...visibleProfiles.map((profile) => profile.id)])))}>Select shown</button>
            <button className="text-button" disabled={running || selectedIds.length === 0} onClick={() => setSelectedIds([])}>Clear</button>
          </div>
        </div>
        <label className="benchmark-search">
          <span>⌕</span>
          <input value={modelFilter} onChange={(event) => setModelFilter(event.target.value)} placeholder="Filter by model or feature" />
          {modelFilter && <button type="button" onClick={() => setModelFilter("")} aria-label="Clear model filter">×</button>}
        </label>
        {profiles.length === 0 ? (
          <div className="benchmark-empty"><strong>No detected models</strong><span>Add launch scripts, then rescan the scripts folder.</span></div>
        ) : (
          <div className="bench-model-groups">
            {modelGroups.map((group) => (
              <section className="bench-model-group" key={group.model}>
                <div className="bench-model-group-heading">
                  <strong>{group.model}</strong>
                  <span>{group.profiles.length} {group.profiles.length === 1 ? "profile" : "profiles"}</span>
                </div>
                <div className="bench-model-list">
                  {group.profiles.map((profile) => {
                    const selected = selectedIds.includes(profile.id);
                    return <label key={profile.id} className={`bench-model ${selected ? "selected" : ""}`}>
                      <input type="checkbox" checked={selected} onChange={() => toggleModel(profile.id)} disabled={running} />
                      <span className="bench-model-check">{selected ? "✓" : ""}</span>
                      <span className="bench-model-name"><b>{profile.prettyFeature}</b><small>{profile.extension.replace(".", "").toUpperCase()} launch profile</small></span>
                    </label>;
                  })}
                </div>
              </section>
            ))}
            {visibleProfiles.length === 0 && <div className="benchmark-empty"><strong>No matching profiles</strong><span>Try another model or feature name.</span></div>}
          </div>
        )}
      </section>

      {/* Prompts */}
      <section className="benchmark-panel">
        <div className="benchmark-panel-heading">
          <div>
            <span className="benchmark-step">02</span>
            <div><h2>Choose prompt set</h2><p>Check the prompts to run; skipped prompts stay saved for later.</p></div>
          </div>
          <div className="benchmark-heading-actions">
            <span className="benchmark-selection-count">{enabledPrompts.length} of {prompts.length} selected</span>
            <button className="text-button" disabled={running || enabledPrompts.length === prompts.length} onClick={() => setPrompts((items) => items.map((prompt) => ({ ...prompt, enabled: true })))}>Select all</button>
            <button className="text-button" disabled={running || enabledPrompts.length === 0} onClick={() => setPrompts((items) => items.map((prompt) => ({ ...prompt, enabled: false })))}>Clear</button>
            <button className="btn small" onClick={addPrompt} disabled={running}>+ Add prompt</button>
          </div>
        </div>
        <div className="benchmark-prompts">
          {prompts.map((p, i) => {
            const expanded = activePromptId === p.id;
            const enabled = p.enabled !== false;
            return <article key={p.id} className={`bench-prompt ${enabled ? "selected" : "excluded"} ${expanded ? "expanded" : ""}`}>
              <div className="bench-prompt-header">
                <label className="bench-prompt-select" title={enabled ? "Included in this run" : "Skipped in this run"}>
                  <input type="checkbox" checked={enabled} onChange={(event) => updatePrompt(i, { enabled: event.target.checked })} disabled={running} aria-label={`${enabled ? "Exclude" : "Include"} ${p.title || `prompt ${i + 1}`}`} />
                  <span>{enabled ? "✓" : ""}</span>
                </label>
                <button className="bench-prompt-toggle" type="button" onClick={() => setActivePromptId(expanded ? null : p.id)}>
                  <span>{String(i + 1).padStart(2, "0")}</span>
                  <span><b>{p.title || `Prompt ${i + 1}`}</b><small>{enabled ? "Included" : "Skipped"} · {p.text.trim() ? `${p.text.trim().length} characters` : "Empty prompt"}</small></span>
                  <i>{expanded ? "−" : "+"}</i>
                </button>
                <button className="icon-button danger" title="Remove prompt" aria-label={`Remove ${p.title || `prompt ${i + 1}`}`} onClick={() => removePrompt(i)} disabled={running || prompts.length <= 1}>×</button>
              </div>
              {expanded && <div className="bench-prompt-editor">
                <label>Prompt title<input type="text" value={p.title} onChange={(event) => updatePrompt(i, { title: event.target.value })} disabled={running} placeholder={`Prompt ${i + 1} title`} /></label>
                <label>Instructions<textarea className="bench-textarea" value={p.text} onChange={(event) => updatePrompt(i, { text: event.target.value })} disabled={running} rows={6} placeholder="Enter the exact prompt sent to each model…" /></label>
              </div>}
            </article>;
          })}
        </div>
      </section>

      {/* Output + timeout */}
      <section className="benchmark-panel benchmark-output-panel">
        <div className="benchmark-panel-heading">
          <div>
            <span className="benchmark-step">03</span>
            <div><h2>Output & limits</h2><p>Choose where artifacts are saved and allow large models enough time to load.</p></div>
          </div>
        </div>
        <div className="benchmark-output-grid">
          <label className="benchmark-output-path">
            <span>Results folder</span>
            <div>
              <input type="text" value={outputDir} onChange={(event) => setOutputDir(event.target.value)} disabled={running} placeholder="Choose where results are written" />
              <button className="btn" onClick={() => void browse()} disabled={running}>Browse…</button>
            </div>
            <small>Organized by model and prompt with the response, extracted code, and metadata.</small>
          </label>
          <label className="benchmark-timeout">
            <span>Per-prompt timeout</span>
            <div><input type="number" min={1} value={timeoutSeconds} onChange={(event) => setTimeoutSeconds(Number(event.target.value))} disabled={running} /><b>seconds</b></div>
          </label>
          <label className="benchmark-timeout">
            <span>Model startup timeout</span>
            <div><input type="number" min={1} value={modelStartTimeoutSeconds} onChange={(event) => setModelStartTimeoutSeconds(Number(event.target.value))} disabled={running} /><b>seconds</b></div>
          </label>
        </div>
      </section>

      {/* Progress grid */}
      {selectedProfiles.length > 0 && enabledPrompts.length > 0 && (
        <section className="benchmark-panel benchmark-progress-panel">
          <div className="benchmark-progress-heading">
            <div><span className={`run-indicator ${running ? "active" : ""}`} /><div><h2>{running ? "Benchmark in progress" : "Run preview"}</h2><p>{finishedJobs} of {totalJobs} evaluations complete</p></div></div>
            <strong>{Math.round(progressPercent)}%</strong>
          </div>
          <div className="benchmark-progress-track"><span style={{ width: `${progressPercent}%` }} /></div>
          <div className="bench-grid-wrap">
            <table className="bench-grid">
              <thead>
                <tr>
                  <th>Model</th>
                  {enabledPrompts.map((p) => (
                    <th key={p.id} title={p.title}>
                      #{prompts.findIndex((prompt) => prompt.id === p.id) + 1}
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
                      {enabledPrompts.map((p) => {
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
        </section>
      )}
    </div>
  );
}
