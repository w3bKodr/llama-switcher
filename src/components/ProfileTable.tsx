import { useEffect, useState } from "react";
import { api } from "../api";
import type { Profile, Status } from "../types";

export function ProfileTable({
  profiles,
  status,
  favoriteIds,
  reorderable = false,
  groupedByModel = false,
  onToggleFavorite,
  onReorder,
  onAction,
  showToast,
}: {
  profiles: Profile[];
  status: Status | null;
  favoriteIds: string[];
  reorderable?: boolean;
  groupedByModel?: boolean;
  onToggleFavorite: (profileId: string) => void;
  onReorder?: (draggedId: string, targetId: string) => void;
  onAction: () => void;
  showToast: (m: string, e?: boolean) => void;
}) {
  const [busy, setBusy] = useState<string | null>(null);
  const [draggedId, setDraggedId] = useState<string | null>(null);
  const [dragTargetId, setDragTargetId] = useState<string | null>(null);

  useEffect(() => {
    if (!draggedId) return;
    const trackDrag = (event: PointerEvent) => {
      const element = document.elementFromPoint(event.clientX, event.clientY) as HTMLElement | null;
      const targetId = element?.closest<HTMLElement>("[data-profile-id]")?.dataset.profileId;
      if (!targetId || targetId === draggedId || targetId === dragTargetId) return;
      setDragTargetId(targetId);
      onReorder?.(draggedId, targetId);
    };
    const finishDrag = () => {
      setDraggedId(null);
      setDragTargetId(null);
    };
    window.addEventListener("pointermove", trackDrag);
    window.addEventListener("pointerup", finishDrag);
    window.addEventListener("pointercancel", finishDrag);
    window.addEventListener("blur", finishDrag);
    return () => {
      window.removeEventListener("pointermove", trackDrag);
      window.removeEventListener("pointerup", finishDrag);
      window.removeEventListener("pointercancel", finishDrag);
      window.removeEventListener("blur", finishDrag);
    };
  }, [dragTargetId, draggedId, onReorder]);

  async function run(profile: Profile) {
    setBusy(profile.id);
    try {
      await api.switchProfile(profile.id);
      showToast(`${profile.alias} started.`);
      onAction();
    } catch (e) {
      showToast(`Could not start ${profile.alias}: ${String(e)}`, true);
    } finally {
      setBusy(null);
    }
  }

  if (profiles.length === 0) {
    return null;
  }

  return (
    <div className={`profile-grid ${reorderable ? "favorite-grid" : ""}`}>
      {profiles.map((profile, index) => {
        const isCurrent = status?.running && status.currentProfileId === profile.id;
        const isFavorite = favoriteIds.includes(profile.id);
        const filename = profile.scriptPath.split(/[\\/]/).pop() ?? profile.scriptPath;
        return (
          <article
            className={`profile-card ${isCurrent ? "current" : ""} ${draggedId === profile.id ? "dragging" : ""} ${dragTargetId === profile.id ? "drag-target" : ""}`}
            key={profile.id}
            data-profile-id={profile.id}
          >
            <div className="profile-card-topline">
              {reorderable && (
                <button
                  className="drag-handle"
                  type="button"
                  title="Drag to reorder"
                  aria-label={`Reorder ${profile.alias}. Position ${index + 1} of ${profiles.length}.`}
                  onPointerDown={(event) => {
                    if (event.button !== 0) return;
                    event.preventDefault();
                    setDraggedId(profile.id);
                    setDragTargetId(null);
                  }}
                  onKeyDown={(event) => {
                    if (event.key === "ArrowUp" || event.key === "ArrowLeft") {
                      if (index > 0) onReorder?.(profile.id, profiles[index - 1].id);
                      event.preventDefault();
                    } else if (event.key === "ArrowDown" || event.key === "ArrowRight") {
                      if (index < profiles.length - 1) onReorder?.(profile.id, profiles[index + 1].id);
                      event.preventDefault();
                    }
                  }}
                >
                  <span>⠿</span><b>{index + 1}</b>
                </button>
              )}
              <div className="profile-heading">
                <strong title={profile.alias}>
                  {groupedByModel ? profile.prettyFeature : profile.prettyModel}
                </strong>
                {!groupedByModel && <span className="feature-pill">{profile.prettyFeature}</span>}
              </div>
              <button
                className={`star-button ${isFavorite ? "selected" : ""}`}
                type="button"
                title={isFavorite ? "Remove from favorites" : "Add to favorites"}
                aria-label={isFavorite ? `Remove ${profile.alias} from favorites` : `Add ${profile.alias} to favorites`}
                aria-pressed={isFavorite}
                onClick={() => onToggleFavorite(profile.id)}
              >
                {isFavorite ? "★" : "☆"}
              </button>
            </div>

            <div className="profile-file" title={profile.scriptPath}>
              <span>{profile.extension.replace(".", "").toUpperCase()}</span>
              <code>{filename}</code>
            </div>

            <div className="profile-card-actions">
              <span className={`profile-state ${isCurrent ? "online" : ""}`}>
                <i />{isCurrent ? "Running now" : "Ready"}
              </span>
              <button
                className={`btn small ${isCurrent ? "" : "primary"}`}
                disabled={busy !== null}
                onClick={() => void run(profile)}
              >
                {busy === profile.id ? "Starting…" : isCurrent ? "Restart" : "Start"}
              </button>
            </div>
          </article>
        );
      })}
    </div>
  );
}
