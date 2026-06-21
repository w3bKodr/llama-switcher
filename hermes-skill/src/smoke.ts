// Optional smoke test: prints status + profiles. Run with valid .env loaded.
//   node --env-file=.env --loader ts-node/esm src/smoke.ts
import { llama_switcher_list_profiles, llama_switcher_status } from "./tools.js";

async function main() {
  const status = await llama_switcher_status();
  console.log("Status:", status);
  const profiles = await llama_switcher_list_profiles();
  console.log(
    "Profiles:",
    profiles.map((p) => `${p.alias} (${p.id})`)
  );
}

main().catch((e) => {
  console.error(String(e));
  process.exit(1);
});
