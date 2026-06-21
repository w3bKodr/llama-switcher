// Thin HTTP client for the Llama Switcher local control API.
//
// All process management lives inside the Tauri/Rust app. This client only
// makes authenticated HTTP calls — it never runs .cmd/.bat/.ps1 files and never
// kills processes itself.

const BASE_URL =
  process.env.LLAMA_SWITCHER_BASE_URL ?? "http://127.0.0.1:47891";
const TOKEN = process.env.LLAMA_SWITCHER_API_TOKEN;

export class LlamaSwitcherError extends Error {}

export async function callLlamaSwitcher<T = unknown>(
  path: string,
  options: RequestInit = {}
): Promise<T> {
  if (!TOKEN) {
    throw new LlamaSwitcherError(
      "LLAMA_SWITCHER_API_TOKEN is not configured."
    );
  }

  let res: Response;
  try {
    res = await fetch(`${BASE_URL}${path}`, {
      ...options,
      headers: {
        Authorization: `Bearer ${TOKEN}`,
        "Content-Type": "application/json",
        ...(options.headers ?? {}),
      },
    });
  } catch {
    // Connection refused / DNS / timeout — the tray app isn't reachable.
    throw new LlamaSwitcherError(
      "Llama Switcher is not reachable. Start the tray app first."
    );
  }

  if (!res.ok) {
    let detail = "";
    try {
      const body = (await res.json()) as { error?: string };
      detail = body?.error ?? "";
    } catch {
      detail = await res.text().catch(() => "");
    }
    throw new LlamaSwitcherError(
      `Llama Switcher API error ${res.status}: ${detail || res.statusText}`
    );
  }

  // Some endpoints (e.g. /open-dashboard) return a tiny ack object.
  return (await res.json()) as T;
}
