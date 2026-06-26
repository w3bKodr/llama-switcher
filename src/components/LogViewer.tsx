import { useLayoutEffect, useRef } from "react";

export function LogViewer({
  text,
  follow = true,
}: {
  text: string;
  follow?: boolean;
}) {
  const ref = useRef<HTMLDivElement | null>(null);
  const pinnedToBottom = useRef(true);

  function updatePinnedState() {
    const el = ref.current;
    if (!el) return;
    const distanceFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
    pinnedToBottom.current = distanceFromBottom < 24;
  }

  useLayoutEffect(() => {
    if (follow && pinnedToBottom.current && ref.current) {
      ref.current.scrollTop = ref.current.scrollHeight;
    }
  }, [follow, text]);

  return (
    <div ref={ref} className="log-view" onScroll={updatePinnedState}>
      {text || "No log content."}
    </div>
  );
}
