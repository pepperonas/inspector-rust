import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "happy-dom",
    include: ["src/**/*.test.{ts,tsx}"],
    // Global React-tree cleanup — see src/test-setup.ts for why this must be
    // central and not left to each file's discipline.
    setupFiles: ["./src/test-setup.ts"],
  },
});
