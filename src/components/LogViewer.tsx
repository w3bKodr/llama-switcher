export function LogViewer({ text }: { text: string }) {
  return <div className="log-view">{text || "No log content."}</div>;
}
