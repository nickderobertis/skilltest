import { fileURLToPath } from "node:url";
import { object } from "./validation.mjs";
/** @param {unknown} rawNeeds @param {string|undefined} macosRequired */
export function requiredResult(rawNeeds, macosRequired) {
  const needs = object(rawNeeds, "needs");
  const policy = object(needs.policy, "policy outcome");
  if (!needs || policy.result !== "success" || !["true", "false"].includes(macosRequired ?? ""))
    throw new Error("classification failed or missing; required context cannot succeed");
  const outcome = object(needs["mac-check"], "Mac outcome").result;
  if (macosRequired === "true" && outcome === "success")
    return "Mac check and bundle installation proofs passed";
  if (macosRequired === "false" && outcome === "skipped")
    return "Mac lane intentionally omitted by semantic classifier; no Mac tests ran";
  throw new Error(`required Mac work ${outcome ?? "missing"}; macosRequired=${macosRequired}`);
}
if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    console.log(
      requiredResult(JSON.parse(process.env.NEEDS ?? "null"), process.env.MACOS_REQUIRED),
    );
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  }
}
