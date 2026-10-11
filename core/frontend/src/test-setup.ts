// Shared vitest setup (wired via `setupFiles` in vitest.config.ts).
//
// With `globals: false`, @testing-library/react cannot auto-register its
// per-test cleanup — a React test file that forgets `afterEach(cleanup)`
// leaves its tree mounted at file end, and React 19's scheduler can then run
// a pending tick AFTER happy-dom tears the environment down → an intermittent
// "window is not defined" crash that fails the whole suite (field-hit in
// useModifierHeld.test.ts). Registering cleanup here covers every file;
// per-file `afterEach(cleanup)` remains harmless (unmounting twice is a no-op).
import { afterEach } from "vitest";
import { cleanup } from "@testing-library/react";
import en from "./locales/en.json";
import { setCatalog } from "./lib/i18n";

// Components translate through `t()`; tests run with the English catalogue
// loaded (a test that checks German text switches with `setCatalog("de", de)`).
setCatalog("en", en);

afterEach(cleanup);
