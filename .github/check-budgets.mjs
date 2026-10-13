import { check } from "@onebudgetspec/sdk";
const result = await check({
  paths: [process.argv[2] ?? ".github/budgets.yaml"],
  cwd: process.cwd(),
});
for (const budget of result.results)
  console.log(`${budget.id}: ${budget.verdict} (${budget.actual} ${budget.unit})`);
if (result.results.some((budget) => budget.verdict !== "within")) process.exitCode = 1;
