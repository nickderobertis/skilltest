// Usage: check-budgets.mjs [budgets.yaml]. Prints each budget's verdict.
// Exit codes: 0 every budget within; 1 any budget over or not measurable.
import { check } from "@onebudgetspec/sdk";
const result = await check({
  paths: [process.argv[2] ?? ".github/budgets.yaml"],
  cwd: process.cwd(),
});
for (const budget of result.results)
  console.log(`${budget.id}: ${budget.verdict} (${budget.actual} ${budget.unit})`);
if (result.results.some((budget) => budget.verdict !== "within")) process.exitCode = 1;
