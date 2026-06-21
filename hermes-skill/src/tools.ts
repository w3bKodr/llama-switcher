// Tool functions exposed to Hermes Agent. Each is a thin wrapper over one
// Llama Switcher control endpoint.

import { callLlamaSwitcher } from "./client.js";
import type {
  Profile,
  ScanResult,
  Status,
  SwitchByAliasInput,
  SwitchByNameInput,
} from "./types.js";

/** Check which llama.cpp profile is currently running. */
export async function llama_switcher_status(): Promise<Status> {
  return callLlamaSwitcher<Status>("/status");
}

/** List all detected llama.cpp startup scripts / profiles. */
export async function llama_switcher_list_profiles(): Promise<Profile[]> {
  return callLlamaSwitcher<Profile[]>("/profiles");
}

/** Rescan the configured scripts folder. */
export async function llama_switcher_rescan(): Promise<ScanResult> {
  return callLlamaSwitcher<ScanResult>("/rescan", { method: "POST" });
}

/**
 * Switch the active server to a profile by human alias (e.g. "Qwen-27B MTP").
 * This is the preferred switch tool. If the alias is ambiguous, the API returns
 * a 409 and this throws with the list of possible matches.
 */
export async function llama_switcher_switch_by_alias(
  input: SwitchByAliasInput
): Promise<Status> {
  return callLlamaSwitcher<Status>("/switch-by-alias", {
    method: "POST",
    body: JSON.stringify({ alias: input.alias }),
  });
}

/** Switch the active server using model + feature. */
export async function llama_switcher_switch_by_name(
  input: SwitchByNameInput
): Promise<Status> {
  return callLlamaSwitcher<Status>("/switch-by-name", {
    method: "POST",
    body: JSON.stringify({ model: input.model, feature: input.feature }),
  });
}

/** Restart the currently running profile. */
export async function llama_switcher_restart(): Promise<Status> {
  return callLlamaSwitcher<Status>("/restart", { method: "POST" });
}

/** Stop the currently running llama.cpp server. */
export async function llama_switcher_stop(): Promise<Status> {
  return callLlamaSwitcher<Status>("/stop", { method: "POST" });
}

/** Open the Llama Switcher dashboard window. */
export async function llama_switcher_open_dashboard(): Promise<{ ok: boolean }> {
  return callLlamaSwitcher<{ ok: boolean }>("/open-dashboard", {
    method: "POST",
  });
}
