// Config for the *child* vitest run used by failure.test.ts. The case file is
// named `*.case.mts` so the outer suite never collects it directly.
import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    include: ["**/*.case.mts"],
  },
});
