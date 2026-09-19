import { useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "../api";
import type {
  BenchmarkPrompt,
  BenchmarkProgress,
  BenchmarkConfig,
  ProfessionalBenchmarkSummary,
  Profile,
} from "../types";

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

function scoreTone(score: number): string {
  if (score < 40) return "score-bad";
  if (score < 60) return "score-warning";
  if (score < 90) return "score-caution";
  return "score-good";
}

type CellState = "pending" | "running" | "done" | "error";
type ModelState = "pending" | "switching" | "ready" | "error";
type RepetitionProgress = { current: number; total: number };
type AcceptanceTotal = { drafted: number; accepted: number };

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
  const [professionalCases, setProfessionalCases] = useState<ProfessionalBenchmarkSummary[]>([]);
  const [selectedProfessionalIds, setSelectedProfessionalIds] = useState<string[]>([]);
  const [outputDir, setOutputDir] = useState("");
  const [timeoutSeconds, setTimeoutSeconds] = useState(300);
  const [modelStartTimeoutSeconds, setModelStartTimeoutSeconds] = useState(300);
  const [gradingTimeoutSeconds, setGradingTimeoutSeconds] = useState(30);
  const [runsPerPrompt, setRunsPerPrompt] = useState(1);
  const [generateHtmlReport, setGenerateHtmlReport] = useState(true);
  const [openReportWhenComplete, setOpenReportWhenComplete] = useState(false);
  const [resumeCompletedRuns, setResumeCompletedRuns] = useState(true);
  const [resumeCount, setResumeCount] = useState(0);
  const [modelFilter, setModelFilter] = useState("");
  const [activePromptId, setActivePromptId] = useState<string | null>(null);

  const [running, setRunning] = useState(false);
  const [pauseState, setPauseState] = useState<"running" | "pausing" | "paused">("running");
  const [cells, setCells] = useState<Record<string, CellState>>({});
  const [durations, setDurations] = useState<Record<string, number>>({});
  const [tps, setTps] = useState<Record<string, number>>({});
  const [repetitionProgress, setRepetitionProgress] = useState<Record<string, RepetitionProgress>>({});
  const [completedRuns, setCompletedRuns] = useState<Record<string, true>>({});
  const [acceptanceTotals, setAcceptanceTotals] = useState<Record<string, AcceptanceTotal>>({});
  const [scores, setScores] = useState<Record<string, number>>({});
  const [gradeCounts, setGradeCounts] = useState<Record<string, { passed: number; total: number }>>({});
  const [reportPath, setReportPath] = useState<string | null>(null);
  const [modelStates, setModelStates] = useState<Record<string, ModelState>>({});
  const [errors, setErrors] = useState<string[]>([]);
  const promptSeq = useRef(3);

  // Load persisted config + detected profiles.
  useEffect(() => {
    (async () => {
      try {
        const [cfg, catalog, profs, isRunning] = await Promise.all([
          api.getBenchmarkConfig(),
          api.getProfessionalBenchmarkCatalog(),
          api.getDetectedProfiles(),
          api.isBenchmarkRunning(),
        ]);
        setProfiles(profs);
        setProfessionalCases(catalog);
        setPrompts(cfg.prompts);
        setActivePromptId(cfg.prompts[0]?.id ?? null);
        setSelectedProfessionalIds(
          cfg.professionalCaseIds
            ? cfg.professionalCaseIds.filter((id) => catalog.some((item) => item.id === id))
            : catalog.map((item) => item.id),
        );
        setOutputDir(cfg.outputDir);
        setTimeoutSeconds(cfg.timeoutSeconds);
        setModelStartTimeoutSeconds(cfg.modelStartTimeoutSeconds ?? 300);
        setGradingTimeoutSeconds(cfg.gradingTimeoutSeconds ?? 30);
        setRunsPerPrompt(cfg.runsPerPrompt ?? 1);
        setGenerateHtmlReport(cfg.generateHtmlReport !== false);
        setOpenReportWhenComplete(cfg.openReportWhenComplete === true);
        setResumeCompletedRuns(cfg.resumeCompletedRuns !== false);
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
          setPauseState("running");
          setCells({});
          setDurations({});
          setTps({});
          setRepetitionProgress({});
          setCompletedRuns({});
          setAcceptanceTotals({});
          setScores({});
          setGradeCounts({});
          setReportPath(null);
          setModelStates({});
          setErrors([]);
        } else if (p.status === "pausing") {
          setPauseState("pausing");
        } else if (p.status === "paused") {
          setPauseState("paused");
        } else if (p.status === "resumed") {
          setPauseState("running");
        } else if (p.status === "finished" || p.status === "cancelled") {
          setRunning(false);
          setPauseState("running");
          if (p.reportPath) {
            setReportPath(p.reportPath);
            if (openReportWhenComplete) {
              void api.openBenchmarkReport(p.reportPath);
            }
          }
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
        const key = cellKey(p.profileId!, p.promptId!);
        if (p.status === "running") {
          setCells((c) => ({ ...c, [key]: "running" }));
          if (p.runIndex != null && p.runCount != null) {
            setRepetitionProgress((items) => ({
              ...items,
              [key]: { current: p.runIndex!, total: p.runCount! },
            }));
          }
        } else if (p.status === "iteration_done" || p.status === "iteration_error") {
          if (p.runIndex != null) {
            setCompletedRuns((items) => ({ ...items, [`${key}::${p.runIndex}`]: true }));
          }
          if (p.status === "iteration_error" && p.message) {
            setErrors((es) => [...es, `${p.alias ?? p.profileId} / ${p.promptId} / run ${p.runIndex ?? "?"}: ${p.message}`]);
          }
        } else if (p.status === "done" || p.status === "error") {
          setCells((c) => ({ ...c, [key]: p.status as CellState }));
        }
        if (p.status === "done" && p.durationSeconds != null) {
          setDurations((d) => ({ ...d, [key]: p.durationSeconds! }));
        }
        if (p.status === "done" && p.tokensPerSecond != null) {
          setTps((t) => ({ ...t, [key]: p.tokensPerSecond! }));
        }
        if ((p.status === "done" || p.status === "error") && p.draftTokens != null && p.acceptedDraftTokens != null) {
          setAcceptanceTotals((items) => ({
            ...items,
            [key]: { drafted: p.draftTokens!, accepted: p.acceptedDraftTokens! },
          }));
        }
        if (p.score != null) {
          setScores((items) => ({ ...items, [key]: p.score! }));
        }
        if (p.passedTests != null && p.totalTests != null) {
          setGradeCounts((items) => ({
            ...items,
            [key]: { passed: p.passedTests!, total: p.totalTests! },
          }));
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
  const enabledProfessionalCases = useMemo(
    () => professionalCases.filter((item) => selectedProfessionalIds.includes(item.id)),
    [professionalCases, selectedProfessionalIds],
  );
  const benchmarkItems = useMemo(
    () => [
      ...enabledPrompts.map((prompt) => ({
        id: prompt.id,
        title: prompt.title,
        kind: "custom" as const,
        difficulty: null as string | null,
      })),
      ...enabledProfessionalCases.map((item) => ({
        id: item.id,
        title: item.title,
        kind: "professional" as const,
        difficulty: item.difficulty,
      })),
    ],
    [enabledPrompts, enabledProfessionalCases],
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
  const totalJobs = selectedProfiles.length * benchmarkItems.length * runsPerPrompt;
  const finishedJobs = Object.keys(completedRuns).length;
  const progressPercent = totalJobs > 0 ? Math.min(100, (finishedJobs / totalJobs) * 100) : 0;
  const acceptanceByModel = useMemo(() => {
    return selectedProfiles.reduce<Record<string, AcceptanceTotal>>((models, profile) => {
      const totals = benchmarkItems.reduce<AcceptanceTotal>((sum, benchmark) => {
        const item = acceptanceTotals[cellKey(profile.id, benchmark.id)];
        return item
          ? { drafted: sum.drafted + item.drafted, accepted: sum.accepted + item.accepted }
          : sum;
      }, { drafted: 0, accepted: 0 });
      models[profile.id] = totals;
      return models;
    }, {});
  }, [acceptanceTotals, benchmarkItems, selectedProfiles]);

  const currentConfig = useMemo<BenchmarkConfig>(() => ({
    profileIds: selectedIds,
    prompts,
    professionalCaseIds: selectedProfessionalIds,
    outputDir,
    timeoutSeconds,
    modelStartTimeoutSeconds,
    gradingTimeoutSeconds,
    runsPerPrompt,
    generateHtmlReport,
    openReportWhenComplete,
    resumeCompletedRuns,
  }), [selectedIds, prompts, selectedProfessionalIds, outputDir, timeoutSeconds,
    modelStartTimeoutSeconds, gradingTimeoutSeconds, runsPerPrompt,
    generateHtmlReport, openReportWhenComplete, resumeCompletedRuns]);

  useEffect(() => {
    if (running || !resumeCompletedRuns || !outputDir.trim()) {
      setResumeCount(0);
      return;
    }
    const timer = window.setTimeout(() => {
      api.getBenchmarkResumeCount(currentConfig).then(setResumeCount).catch(() => setResumeCount(0));
    }, 250);
    return () => window.clearTimeout(timer);
  }, [currentConfig, outputDir, resumeCompletedRuns, running]);

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

  async function run(startFresh = false) {
    if (selectedIds.length === 0) return showToast("Select at least one model.", true);
    if (benchmarkItems.length === 0) return showToast("Select at least one custom or professional benchmark.", true);
    if (!outputDir.trim()) return showToast("Choose an output folder.", true);
    try {
      await api.runBenchmark(currentConfig, startFresh);
      showToast(startFresh ? "New benchmark started." : "Benchmark resumed.");
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

  async function togglePause() {
    try {
      if (pauseState === "paused") {
        await api.resumeBenchmark();
      } else {
        await api.pauseBenchmark();
      }
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
          <div><b>{benchmarkItems.length}</b><span>Benchmarks</span></div>
          <div><b>{totalJobs}</b><span>Total runs</span></div>
          <div><b>{runsPerPrompt}×</b><span>Each prompt</span></div>
        </div>
        <div className="benchmark-primary-action">
          {running ? (
            <div className="benchmark-run-controls">
              <button className="btn benchmark-run-button" disabled={pauseState === "pausing"} onClick={() => void togglePause()}>
                <span>{pauseState === "paused" ? "▶" : "Ⅱ"}</span> {pauseState === "paused" ? "Resume" : pauseState === "pausing" ? "Pausing…" : "Pause"}
              </button>
              <button className="btn danger benchmark-run-button" onClick={() => void cancel()}><span>■</span> Cancel</button>
            </div>
          ) : resumeCount > 0 ? (
            <div className="benchmark-start-options">
              <button className="btn primary benchmark-run-button" onClick={() => void run(false)}><span>▶</span> Resume benchmark</button>
              <button className="btn benchmark-new-button" onClick={() => void run(true)}>Start new benchmark</button>
            </div>
          ) : (
            <button className="btn primary benchmark-run-button" onClick={() => void run(true)}><span>▶</span> Run benchmark</button>
          )}
          <small>{running ? pauseState === "paused" ? `Paused · ${finishedJobs} of ${totalJobs} completed` : pauseState === "pausing" ? "Pausing after the active evaluation…" : `${finishedJobs} of ${totalJobs} completed` : resumeCount > 0 ? `${resumeCount} verified saved · ${totalJobs - resumeCount} remaining` : `${totalJobs} queued evaluations`}</small>
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
            <div><h2>Choose benchmarks</h2><p>Use deterministic professional cases, custom performance prompts, or both.</p></div>
          </div>
          <div className="benchmark-heading-actions">
            <span className="benchmark-selection-count">{benchmarkItems.length} selected</span>
            <button className="text-button" disabled={running || enabledPrompts.length === prompts.length} onClick={() => setPrompts((items) => items.map((prompt) => ({ ...prompt, enabled: true })))}>Select custom tests</button>
            <button className="text-button" disabled={running || enabledPrompts.length === 0} onClick={() => setPrompts((items) => items.map((prompt) => ({ ...prompt, enabled: false })))}>Clear custom tests</button>
            <button className="btn small" onClick={addPrompt} disabled={running}>+ Add custom test</button>
          </div>
        </div>
        <details className="benchmark-subsection benchmark-expander" open>
          <summary className="benchmark-subsection-heading">
            <div><strong>Professional tests</strong><small>18 current-generation coding tasks with hidden tests. Suite v{professionalCases[0]?.suiteVersion ?? 2}.</small></div>
            <span>{selectedProfessionalIds.length} of {professionalCases.length} selected</span>
          </summary>
          <div className="benchmark-prompts">
            {professionalCases.map((item, i) => {
              const selected = selectedProfessionalIds.includes(item.id);
              const expanded = activePromptId === item.id;
              return <article key={item.id} className={"bench-prompt professional-prompt " + (selected ? "selected" : "excluded") + (expanded ? " expanded" : "")}>
                <div className="bench-prompt-header">
                  <label className="bench-prompt-select" title={selected ? "Included in this run" : "Skipped in this run"}>
                    <input type="checkbox" checked={selected} onChange={(event) => setSelectedProfessionalIds((ids) => event.target.checked ? [...ids, item.id] : ids.filter((id) => id !== item.id))} disabled={running} aria-label={(selected ? "Exclude " : "Include ") + item.title} />
                    <span>{selected ? "✓" : ""}</span>
                  </label>
                  <button className="bench-prompt-toggle" type="button" onClick={() => setActivePromptId(expanded ? null : item.id)}>
                    <span>{String(i + 1).padStart(2, "0")}</span>
                    <span><b>{item.title}</b><small>{item.difficulty.toUpperCase()} · {item.totalTestCount} deterministic tests · {selected ? "Included" : "Skipped"}</small></span>
                    <i>{expanded ? "−" : "+"}</i>
                  </button>
                  <span className={"benchmark-difficulty " + item.difficulty}>{item.difficulty}</span>
                </div>
                {expanded && <div className="bench-prompt-editor professional-preview">
                  <p>{item.description}</p>
                  <code>{item.functionSignature}</code>
                  <pre>{item.prompt}</pre>
                </div>}
              </article>;
            })}
          </div>
        </details>
        <details className="benchmark-subsection benchmark-expander">
          <summary className="benchmark-subsection-heading">
            <div><strong>Custom tests</strong><small>Editable generation and performance tests; correctness is not graded.</small></div>
            <span>{enabledPrompts.length} of {prompts.length} selected</span>
          </summary>
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
        </details>
      </section>

      {/* Output + timeout */}
      <section className="benchmark-panel benchmark-output-panel">
        <div className="benchmark-panel-heading">
          <div>
            <span className="benchmark-step">03</span>
            <div><h2>Run setup & output</h2><p>Set repetition count, time limits, and where every result is saved.</p></div>
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
            <span>Runs per prompt</span>
            <div><input type="number" min={1} max={100} step={1} value={runsPerPrompt} onChange={(event) => setRunsPerPrompt(Math.max(1, Math.min(100, Math.trunc(Number(event.target.value) || 1))))} disabled={running} /><b>times</b></div>
          </label>
          <label className="benchmark-timeout">
            <span>Per-test generation timeout</span>
            <div><input type="number" min={1} value={timeoutSeconds} onChange={(event) => setTimeoutSeconds(Math.max(1, Number(event.target.value) || 300))} disabled={running} /><b>seconds</b></div>
          </label>
          <label className="benchmark-timeout">
            <span>Model startup timeout</span>
            <div><input type="number" min={1} value={modelStartTimeoutSeconds} onChange={(event) => setModelStartTimeoutSeconds(Number(event.target.value))} disabled={running} /><b>seconds</b></div>
          </label>
          <label className="benchmark-timeout">
            <span>Grading limit</span>
            <div><input type="number" min={1} max={300} value={gradingTimeoutSeconds} onChange={(event) => setGradingTimeoutSeconds(Math.max(1, Math.min(300, Number(event.target.value) || 30)))} disabled={running} /><b>seconds</b></div>
          </label>
          <label className="benchmark-report-option"><span>Report</span><span><input type="checkbox" checked={generateHtmlReport} onChange={(event) => setGenerateHtmlReport(event.target.checked)} disabled={running} /> Generate offline HTML charts</span></label>
          <label className="benchmark-report-option"><span>After completion</span><span><input type="checkbox" checked={openReportWhenComplete} onChange={(event) => setOpenReportWhenComplete(event.target.checked)} disabled={running || !generateHtmlReport} /> Open report automatically</span></label>
          <label className="benchmark-report-option"><span>Crash recovery</span><span><input type="checkbox" checked={resumeCompletedRuns} onChange={(event) => setResumeCompletedRuns(event.target.checked)} disabled={running} /> Resume verified completed runs</span><small>After a crash, start the same benchmark with the same results folder. Finished iterations are skipped; incomplete or changed prompts run again.</small></label>
        </div>
        {reportPath && !running && <div className="benchmark-report-link"><span>Report ready</span><button className="btn small" onClick={() => void api.openBenchmarkReport(reportPath)}>Open benchmark-report.html</button><code>{reportPath}</code></div>}
      </section>

      {/* Progress grid */}
      {selectedProfiles.length > 0 && benchmarkItems.length > 0 && (
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
                  {benchmarkItems.map((p, index) => (
                    <th key={p.id} title={p.title}>
                      #{index + 1} {p.kind === "professional" ? <small className="benchmark-grid-difficulty">{p.difficulty}</small> : null}
                    </th>
                  ))}
                  <th title="Accepted speculative draft tokens divided by all drafted tokens for this model">Weighted spec acceptance</th>
                  <th></th>
                </tr>
              </thead>
              <tbody>
                {selectedProfiles.map((prof) => {
                  const ms = modelStates[prof.id] ?? "pending";
                  const modelAcceptance = acceptanceByModel[prof.id];
                   const modelComplete = benchmarkItems.every((benchmark) => {
                     const state = cells[cellKey(prof.id, benchmark.id)];
                    return state === "done" || state === "error";
                  });
                  return (
                    <tr key={prof.id}>
                      <td>
                        {prof.alias}{" "}
                        {ms === "switching" && <span className="badge yellow">switching…</span>}
                        {ms === "error" && <span className="badge red">error</span>}
                      </td>
                      {benchmarkItems.map((p) => {
                        const key = cellKey(prof.id, p.id);
                        const cs = cells[key] ?? "pending";
                        const dur = durations[key];
                        const speed = tps[key];
                        const repeat = repetitionProgress[key];
                        const score = scores[key];
                        const count = gradeCounts[key];
                        return (
                          <td key={p.id} className={`bench-cell ${cs}`} title={cs}>
                            {cs === "done" ? (
                              <div className="bench-cell-done">
                                {score != null && <b className={`bench-grade-score ${scoreTone(score)}`}>{score.toFixed(0)}%</b>}
                                {count && <small className="bench-grade-count">{count.passed}/{count.total} tests</small>}
                                <span>{dur != null ? `avg ${formatHMS(dur)}` : "✓"}</span>
                                {speed != null && (
                                  <span className="bench-tps">{speed.toFixed(1)} avg tk/s</span>
                                )}
                              </div>
                            ) : cs === "running" && repeat ? (
                              <div className="bench-cell-running"><span className="spinner" /><b>Run {repeat.current}/{repeat.total}</b></div>
                            ) : (
                              CELL_ICON[cs]
                            )}
                          </td>
                        );
                      })}
                      <td className="bench-model-acceptance">
                        {modelAcceptance?.drafted > 0 ? (
                          <><b>{((modelAcceptance.accepted / modelAcceptance.drafted) * 100).toFixed(1)}%</b><small>{modelAcceptance.accepted.toLocaleString()} / {modelAcceptance.drafted.toLocaleString()} tokens</small></>
                        ) : (
                          <><b>—</b><small>{modelComplete ? "No speculative data" : "Awaiting results"}</small></>
                        )}
                      </td>
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
