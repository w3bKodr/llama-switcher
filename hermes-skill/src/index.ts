// Entry point for the Llama Switcher Hermes skill.
//
// Exports:
//   - The individual tool functions (for direct import).
//   - A `tools` registry array (name, description, JSON input schema, handler)
//     that a Hermes Agent runtime can iterate over to register the skill.
//
// Hermes Agent controls Llama Switcher ONLY through these tools, which call the
// local HTTP API. The skill never executes scripts or kills processes.

import {
  llama_switcher_list_profiles,
  llama_switcher_open_dashboard,
  llama_switcher_rescan,
  llama_switcher_restart,
  llama_switcher_status,
  llama_switcher_stop,
  llama_switcher_switch_by_alias,
  llama_switcher_switch_by_name,
} from "./tools.js";

export * from "./tools.js";
export * from "./types.js";
export { LlamaSwitcherError } from "./client.js";

export interface HermesTool {
  name: string;
  description: string;
  inputSchema: Record<string, unknown>;
  handler: (input: any) => Promise<unknown>;
}

const emptySchema = { type: "object", properties: {}, additionalProperties: false };

export const tools: HermesTool[] = [
  {
    name: "llama_switcher_status",
    description:
      "Check which llama.cpp server profile is currently running, including health, PID, and server port.",
    inputSchema: emptySchema,
    handler: () => llama_switcher_status(),
  },
  {
    name: "llama_switcher_list_profiles",
    description:
      "List detected llama.cpp profiles from the configured scripts folder.",
    inputSchema: emptySchema,
    handler: () => llama_switcher_list_profiles(),
  },
  {
    name: "llama_switcher_rescan",
    description:
      "Rescan the scripts folder for llama.cpp startup files after adding or removing them.",
    inputSchema: emptySchema,
    handler: () => llama_switcher_rescan(),
  },
  {
    name: "llama_switcher_switch_by_alias",
    description:
      "Switch to a llama.cpp profile using a human-readable alias such as 'Qwen-27B MTP'. Preferred switch tool.",
    inputSchema: {
      type: "object",
      properties: { alias: { type: "string" } },
      required: ["alias"],
      additionalProperties: false,
    },
    handler: (input: { alias: string }) =>
      llama_switcher_switch_by_alias(input),
  },
  {
    name: "llama_switcher_switch_by_name",
    description:
      "Switch to a llama.cpp profile using separate model and feature values, e.g. model 'Qwen-27B', feature 'MTP'.",
    inputSchema: {
      type: "object",
      properties: {
        model: { type: "string" },
        feature: { type: "string" },
      },
      required: ["model", "feature"],
      additionalProperties: false,
    },
    handler: (input: { model: string; feature: string }) =>
      llama_switcher_switch_by_name(input),
  },
  {
    name: "llama_switcher_restart",
    description: "Restart the currently running llama.cpp server profile.",
    inputSchema: emptySchema,
    handler: () => llama_switcher_restart(),
  },
  {
    name: "llama_switcher_stop",
    description: "Stop the currently running llama.cpp server.",
    inputSchema: emptySchema,
    handler: () => llama_switcher_stop(),
  },
  {
    name: "llama_switcher_open_dashboard",
    description: "Open the Llama Switcher dashboard window.",
    inputSchema: emptySchema,
    handler: () => llama_switcher_open_dashboard(),
  },
];

export default tools;
