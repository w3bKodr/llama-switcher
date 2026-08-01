import { Fragment, useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { Profile, Status } from "../types";

interface DragPreview {
  id: string;
  left: number;
  top: number;
  width: number;
  height: number;
  offsetX: number;
  offsetY: number;
}

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
  const [drag, setDrag] = useState<DragPreview | null>(null);
  const gridRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!drag) return;
    document.body.classList.add("profile-drag-active");

    const animateReorder = (draggedId: string, targetId: string) => {
      const previousTops = new Map<string, number>();
      gridRef.current?.querySelectorAll<HTMLElement>(".profile-card:not(.dragging)").forEach((card) => {
        const id = card.dataset.profileId;
        if (id) previousTops.set(id, card.getBoundingClientRect().top);
      });

      onReorder?.(draggedId, targetId);
      window.requestAnimationFrame(() => {
        gridRef.current?.querySelectorAll<HTMLElement>(".profile-card:not(.dragging)").forEach((card) => {
          const id = card.dataset.profileId;
          const previousTop = id ? previousTops.get(id) : undefined;
          if (previousTop === undefined) return;
          const delta = previousTop - card.getBoundingClientRect().top;
          if (Math.abs(delta) < 1) return;
          card.animate(
            [{ transform: `translateY(${delta}px)` }, { transform: "translateY(0)" }],
            { duration: 190, easing: "cubic-bezier(0.2, 0.8, 0.2, 1)" },
          );
        });
      });
    };

    const trackDrag = (event: PointerEvent) => {
      setDrag((current) => current ? {
        ...current,
        left: event.clientX - current.offsetX,
        top: event.clientY - current.offsetY,
      } : current);

      const cards = Array.from(
        gridRef.current?.querySelectorAll<HTMLElement>(".profile-card:not(.dragging)") ?? [],
      );
      const target = cards.reduce<{ card: HTMLElement; distance: number } | null>((closest, card) => {
        const rect = card.getBoundingClientRect();
        const distance = Math.abs(event.clientY - (rect.top + rect.height / 2));
        return !closest || distance < closest.distance ? { card, distance } : closest;
      }, null)?.card;
      const targetId = target?.dataset.profileId;
      if (!target || !targetId) return;

      const draggedIndex = profiles.findIndex((profile) => profile.id === drag.id);
      const targetIndex = profiles.findIndex((profile) => profile.id === targetId);
      const targetRect = target.getBoundingClientRect();
      const crossedTarget = targetIndex < draggedIndex
        ? event.clientY < targetRect.top + targetRect.height / 2
        : targetIndex > draggedIndex && event.clientY > targetRect.top + targetRect.height / 2;
      if (!crossedTarget) return;

      animateReorder(drag.id, targetId);

      const scroller = gridRef.current?.closest<HTMLElement>(".content");
      if (scroller) {
        const bounds = scroller.getBoundingClientRect();
        if (event.clientY < bounds.top + 52) scroller.scrollBy({ top: -18 });
        else if (event.clientY > bounds.bottom - 52) scroller.scrollBy({ top: 18 });
      }
    };
    const finishDrag = () => {
      setDrag(null);
      document.body.classList.remove("profile-drag-active");
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
      document.body.classList.remove("profile-drag-active");
    };
  }, [drag?.id, onReorder, profiles]);

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
    <div ref={gridRef} className={`profile-grid ${reorderable ? "favorite-grid" : ""}`}>
      {profiles.map((profile, index) => {
        const isCurrent = status?.running && status.currentProfileId === profile.id;
        const isFavorite = favoriteIds.includes(profile.id);
        const isDragged = drag?.id === profile.id;
        const filename = profile.scriptPath.split(/[\\/]/).pop() ?? profile.scriptPath;
        return (
          <Fragment key={profile.id}>
            {isDragged && drag && (
              <div className="profile-drop-slot" style={{ height: drag.height }} aria-hidden="true">
                <span>Drop at position {index + 1}</span>
              </div>
            )}
            <article
              className={`profile-card ${isCurrent ? "current" : ""} ${isDragged ? "dragging" : ""}`}
              data-profile-id={profile.id}
              style={isDragged && drag ? {
                left: drag.left,
                top: drag.top,
                width: drag.width,
                height: drag.height,
              } : undefined}
            >
            {isDragged && <span className="drag-position-badge">Position {index + 1}</span>}
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
                    const card = event.currentTarget.closest<HTMLElement>(".profile-card");
                    if (!card) return;
                    const bounds = card.getBoundingClientRect();
                    document.body.classList.add("profile-drag-active");
                    setDrag({
                      id: profile.id,
                      left: bounds.left,
                      top: bounds.top,
                      width: bounds.width,
                      height: bounds.height,
                      offsetX: event.clientX - bounds.left,
                      offsetY: event.clientY - bounds.top,
                    });
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
          </Fragment>
        );
      })}
    </div>
  );
}
