import { useEffect, useMemo, useRef, useState } from "react";
import { api } from "../api";
import { ProfileTable } from "../components/ProfileTable";
import type { ScanResult, Status } from "../types";

export function reorderProfileIds(ids: string[], draggedId: string, targetId: string) {
  const next = [...ids];
  const from = next.indexOf(draggedId);
  const to = next.indexOf(targetId);
  if (from < 0 || to < 0 || from === to) return ids;
  next.splice(from, 1);
  next.splice(to, 0, draggedId);
  return next;
}

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
  const [favoriteIds, setFavoriteIds] = useState<string[]>([]);
  const [filter, setFilter] = useState("");
  const [busy, setBusy] = useState(false);
  const saveQueue = useRef<Promise<unknown>>(Promise.resolve());

  async function load() {
    try {
      const [result, settings] = await Promise.all([api.getScanResult(), api.getSettings()]);
      setScan(result);
      const detectedIds = new Set(result.profiles.map((profile) => profile.id));
      setFavoriteIds((settings.favoriteProfileIds ?? []).filter((id) => detectedIds.has(id)));
    } catch (e) {
      showToast(`Failed to load scripts: ${String(e)}`, true);
    }
  }

  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function persistFavorites(next: string[]) {
    setFavoriteIds(next);
    saveQueue.current = saveQueue.current
      .catch(() => undefined)
      .then(() => api.saveFavoriteProfiles(next))
      .catch((error) => {
        showToast(`Could not save favorites: ${String(error)}`, true);
      });
  }

  function toggleFavorite(profileId: string) {
    persistFavorites(
      favoriteIds.includes(profileId)
        ? favoriteIds.filter((id) => id !== profileId)
        : [...favoriteIds, profileId],
    );
  }

  function reorderFavorites(draggedId: string, targetId: string) {
    const next = reorderProfileIds(favoriteIds, draggedId, targetId);
    if (next === favoriteIds) return;
    persistFavorites(next);
  }

  async function rescan() {
    setBusy(true);
    try {
      const result = await api.rescanScripts();
      setScan(result);
      showToast(`Found ${result.profiles.length} launch profiles.`);
    } catch (e) {
      showToast(`Rescan failed: ${String(e)}`, true);
    } finally {
      setBusy(false);
    }
  }

  const models = useMemo(
    () => new Set((scan?.profiles ?? []).map((profile) => profile.prettyModel)).size,
    [scan],
  );
  const normalizedFilter = filter.trim().toLowerCase();
  const matches = (scan?.profiles ?? []).filter((profile) =>
    !normalizedFilter
      || `${profile.alias} ${profile.prettyModel} ${profile.prettyFeature}`.toLowerCase().includes(normalizedFilter),
  );
  const profileById = new Map((scan?.profiles ?? []).map((profile) => [profile.id, profile]));
  const favorites = favoriteIds
    .map((id) => profileById.get(id))
    .filter((profile): profile is NonNullable<typeof profile> => !!profile)
    .filter((profile) => matches.some((match) => match.id === profile.id));
  const otherProfiles = matches.filter((profile) => !favoriteIds.includes(profile.id));
  const modelGroups = otherProfiles.reduce<Array<{ model: string; profiles: typeof otherProfiles }>>(
    (groups, profile) => {
      const existing = groups.find((group) => group.model === profile.prettyModel);
      if (existing) existing.profiles.push(profile);
      else groups.push({ model: profile.prettyModel, profiles: [profile] });
      return groups;
    },
    [],
  );

  return (
    <div className="scripts-page">
      <section className="scripts-hero">
        <div>
          <span className="status-kicker">Profile library</span>
          <h1>Detected Scripts</h1>
          <p>Launch and organize every local model profile from one place.</p>
        </div>
        <div className="scripts-stats" aria-label="Script library summary">
          <div><b>{scan?.profiles.length ?? 0}</b><span>Profiles</span></div>
          <div><b>{models}</b><span>Models</span></div>
          <div><b>{favoriteIds.filter((id) => profileById.has(id)).length}</b><span>Starred</span></div>
        </div>
      </section>

      <div className="scripts-toolbar">
        <label className="script-search">
          <span aria-hidden="true">⌕</span>
          <input
            type="text"
            placeholder="Search models or features"
            value={filter}
            onChange={(event) => setFilter(event.target.value)}
          />
          {filter && <button type="button" onClick={() => setFilter("")} aria-label="Clear search">×</button>}
        </label>
        <span className="scan-time">{scan ? `Scanned ${scan.scannedAt}` : "Loading profiles…"}</span>
        <button className="btn" disabled={busy} onClick={() => void rescan()}>
          {busy ? "Rescanning…" : "↻ Rescan folder"}
        </button>
      </div>

      {favorites.length > 0 && (
        <section className="profile-section favorites-section">
          <div className="section-heading">
            <div><span className="section-icon">★</span><strong>Favorites</strong></div>
            <span>Drag cards to set their order</span>
          </div>
          <ProfileTable
            profiles={favorites}
            status={status}
            favoriteIds={favoriteIds}
            reorderable
            onToggleFavorite={toggleFavorite}
            onReorder={reorderFavorites}
            onAction={onAction}
            showToast={showToast}
          />
        </section>
      )}

      {scan && (otherProfiles.length > 0 || favorites.length === 0) && (
        <section className="profile-section">
          <div className="section-heading">
            <div><span className="section-icon neutral">▦</span><strong>{favorites.length ? "All other profiles" : "All profiles"}</strong></div>
            <span>{otherProfiles.length} shown</span>
          </div>
          <div className="model-categories">
            {modelGroups.map((group) => (
              <section className="model-category" key={group.model}>
                <div className="model-category-heading">
                  <strong>{group.model}</strong>
                  <span>{group.profiles.length} {group.profiles.length === 1 ? "feature" : "features"}</span>
                </div>
                <ProfileTable
                  profiles={group.profiles}
                  status={status}
                  favoriteIds={favoriteIds}
                  groupedByModel
                  onToggleFavorite={toggleFavorite}
                  onAction={onAction}
                  showToast={showToast}
                />
              </section>
            ))}
          </div>
          {matches.length === 0 && (
            <div className="empty-scripts">
              <span>⌕</span>
              <strong>No matching profiles</strong>
              <p>Try a different model or feature name.</p>
            </div>
          )}
        </section>
      )}

      {scan && scan.ignoredFiles.length > 0 && (
        <details className="ignored-scripts">
          <summary>
            <span>Ignored files</span>
            <b>{scan.ignoredFiles.length}</b>
          </summary>
          <div className="ignored-list">
            {scan.ignoredFiles.map((file) => (
              <div key={file.filename}>
                <code>{file.filename}</code><span>{file.reason}</span>
              </div>
            ))}
          </div>
        </details>
      )}
    </div>
  );
}
