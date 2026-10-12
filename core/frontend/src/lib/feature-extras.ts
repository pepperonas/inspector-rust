/**
 * The hand-maintained rows of the in-app **Features tab** that are NOT derived
 * from the `CommandDoc` registry: non-command search-bar features, the hidden
 * triggers (games / bpm / equalizer / 2fa), and the in-popup / preview actions.
 *
 * Extracted from `FeaturesPanel.tsx` into data so a test can **guarantee every
 * hidden trigger is documented** (`feature-extras.test.ts`) — the class of
 * feature that has no automatic gate (real commands are covered by
 * `commandDocs.test.ts` + `gen-docs`). When you add a hidden trigger (a new
 * game, visualizer, …) add it to `HIDDEN_TRIGGERS` **and** to the matching row
 * list, or the test fails by name.
 *
 * The visible texts live in the catalogue (`fx.<id>.name|trigger|note`) and are
 * read through getters, so the tab follows the app language; literal triggers
 * (what you actually type) stay as they are.
 */
import { t } from "./i18n";
import { IS_MAC, formatHotkey } from "./platform";

const MOD = IS_MAC ? "Meta" : "Ctrl"; // Cmd on macOS, Ctrl elsewhere

export interface FeatureRow {
  /** Human name of the feature. */
  name: string;
  /** Pre-formatted shortcut, or the literal text the user types. */
  trigger: string;
  /** True → render `trigger` as a typed-command chip, not a key chip. */
  typed?: boolean;
  /** Short "how to use" note; omitted when the trigger speaks for itself. */
  note?: string;
  /** For hidden triggers / games: the exact word the completeness test checks. */
  keyword?: string;
}

/** Search-bar features that aren't power commands (no `CommandDoc`). */
export const NON_COMMAND_FEATURES: FeatureRow[] = [
  { get name() { return t("fx.ai_prompts.name"); }, trigger: "ai… (aiplan · aireview · aifrontend · aibanana · …)", typed: true, get note() { return t("fx.ai_prompts.note"); } },
  { get name() { return t("fx.snippet_groups.name"); }, get trigger() { return t("fx.snippet_groups.trigger"); }, typed: false, get note() { return t("fx.snippet_groups.note"); } },
  { get name() { return t("fx.snippet_storage.name"); }, get trigger() { return t("fx.snippet_storage.trigger"); }, typed: false, get note() { return t("fx.snippet_storage.note"); } },
  { get name() { return t("fx.social_download.name"); }, get trigger() { return t("fx.social_download.trigger"); }, typed: true, get note() { return t("fx.social_download.note"); } },
  { get name() { return t("fx.trim_download.name"); }, get trigger() { return t("fx.trim_download.trigger"); }, typed: false, get note() { return t("fx.trim_download.note"); } },
  { get name() { return t("fx.link_grabber.name"); }, get trigger() { return t("fx.link_grabber.trigger"); }, typed: true, get note() { return t("fx.link_grabber.note"); } },
  { get name() { return t("fx.calculator.name"); }, trigger: "2+2 · sqrt(144) · 0xff & 1", typed: true, get note() { return t("fx.calculator.note"); } },
  { get name() { return t("fx.converter.name"); }, trigger: "5 km in mi · 0xff in dec · 1700000000 as date", typed: true, get note() { return t("fx.converter.note"); } },
  { get name() { return t("fx.colour.name"); }, trigger: "#hex · rgb(…) · hsl(…)", typed: true, get note() { return t("fx.colour.note"); } },
];

/** Hidden triggers — an exact word, deliberately NOT in autocomplete/`COMMANDS`. */
export const HIDDEN_TRIGGER_FEATURES: FeatureRow[] = [
  { get name() { return t("fx.twofa.name"); }, trigger: "2fa", typed: true, keyword: "2fa", get note() { return t("fx.twofa.note"); } },
  { get name() { return t("fx.totp_code.name"); }, trigger: "otp <issuer> · 2fa <issuer>", typed: true, keyword: "otp", get note() { return t("fx.totp_code.note"); } },
  { get name() { return t("fx.bpm.name"); }, trigger: "bpm", typed: true, keyword: "bpm", get note() { return t("fx.bpm.note"); } },
  { get name() { return t("fx.equalizer.name"); }, trigger: "equalizer · eq", typed: true, keyword: "equalizer", get note() { return t("fx.equalizer.note"); } },
  { get name() { return t("fx.app_launcher.name"); }, get trigger() { return t("fx.app_launcher.trigger"); }, typed: true, get note() { return t("fx.app_launcher.note"); } },
];

/** In-popup & preview actions (mouse / keyboard on the popup itself). */
export const IN_POPUP_ACTIONS: FeatureRow[] = [
  { get name() { return t("fx.paste.name"); }, trigger: formatHotkey("Enter") },
  { get name() { return t("fx.settings_search.name"); }, get trigger() { return t("fx.settings_search.trigger"); }, get note() { return t("fx.settings_search.note"); } },
  { get name() { return t("fx.arg_complete.name"); }, trigger: "Tab · Shift+Tab · →", get note() { return t("fx.arg_complete.note"); } },
  { get name() { return t("fx.paste_formatted.name"); }, trigger: formatHotkey("Shift+Enter"), get note() { return t("fx.paste_formatted.note"); } },
  { get name() { return t("fx.navigate.name"); }, trigger: "↑ ↓ · Esc", get note() { return t("fx.navigate.note"); } },
  { get name() { return t("fx.volume.name"); }, trigger: formatHotkey("Shift+ArrowUp") + " / " + formatHotkey("Shift+ArrowDown"), get note() { return t("fx.volume.note"); } },
  { get name() { return t("fx.pin_clip.name"); }, get trigger() { return t("fx.pin_clip.trigger"); }, get note() { return t("fx.pin_clip.note"); } },
  { get name() { return t("fx.sleep_status.name"); }, get trigger() { return t("fx.sleep_status.trigger"); }, get note() { return t("fx.sleep_status.note"); } },
  { get name() { return t("fx.pinned_only.name"); }, get trigger() { return t("fx.pinned_only.trigger"); }, get note() { return t("fx.pinned_only.note"); } },
  { get name() { return t("fx.lineage.name"); }, get trigger() { return t("fx.lineage.trigger"); }, get note() { return t("fx.lineage.note"); } },
  { get name() { return t("fx.formatting.name"); }, get trigger() { return t("fx.formatting.trigger", { key: IS_MAC ? "⌘" : "Ctrl" }); }, get note() { return t("fx.formatting.note"); } },
  { get name() { return t("fx.typed_text.name"); }, get trigger() { return t("fx.typed_text.trigger"); }, get note() { return t("fx.typed_text.note"); } },
  { get name() { return t("fx.smart_actions.name"); }, get trigger() { return t("fx.smart_actions.trigger"); }, get note() { return t("fx.smart_actions.note"); } },
  { get name() { return t("fx.delete.name"); }, get trigger() { return t("fx.delete.trigger"); }, get note() { return t("fx.delete.note"); } },
  { get name() { return t("fx.cutout.name"); }, trigger: formatHotkey(`${MOD}+KeyB`), get note() { return t("fx.cutout.note"); } },
  { get name() { return t("fx.annotate.name"); }, get trigger() { return t("fx.annotate.trigger"); }, get note() { return t("fx.annotate.note"); } },
  { get name() { return t("fx.pin_screenshot.name"); }, get trigger() { return t("fx.pin_screenshot.trigger"); }, get note() { return t("fx.pin_screenshot.note"); } },
  { get name() { return t("fx.transforms.name"); }, trigger: formatHotkey(`${MOD}+1`) + "…" + formatHotkey(`${MOD}+9`), get note() { return t("fx.transforms.note"); } },
  { get name() { return t("fx.recolor.name"); }, get trigger() { return t("fx.recolor.trigger"); }, get note() { return t("fx.recolor.note"); } },
  { get name() { return t("fx.save_note.name"); }, get trigger() { return t("fx.save_note.trigger"); }, get note() { return t("fx.save_note.note"); } },
];

/** Hidden games — exact word into the search field. */
export const HIDDEN_GAMES: FeatureRow[] = [
  { get name() { return t("fx.x.name"); }, trigger: "x!", typed: true, keyword: "x!", get note() { return t("fx.x.note"); } },
  { get name() { return t("fx.xx.name"); }, trigger: "x!!", typed: true, keyword: "x!!", get note() { return t("fx.xx.note"); } },
  { get name() { return t("fx.pong.name"); }, trigger: "getshaky", typed: true, keyword: "getshaky" },
  { get name() { return t("fx.snake_walls.name"); }, trigger: "rockthebox", typed: true, keyword: "rockthebox" },
  { get name() { return t("fx.snake_wrap.name"); }, trigger: "rockthabox", typed: true, keyword: "rockthabox" },
  { get name() { return t("fx.invaders.name"); }, trigger: "spacer", typed: true, keyword: "spacer" },
  { get name() { return t("fx.flappy.name"); }, trigger: "learningtofly", typed: true, keyword: "learningtofly" },
];

/**
 * Every hidden trigger that MUST have a Features-tab row. `feature-extras.test.ts`
 * asserts each appears (by `keyword`) in `HIDDEN_TRIGGER_FEATURES` ∪ `HIDDEN_GAMES`.
 * `opener` is intentionally omitted (an undocumented easter egg); `shazam` is a
 * real `COMMANDS` entry, so it's covered by the `CommandDoc` gate instead.
 */
export const HIDDEN_TRIGGERS: readonly string[] = [
  "x!",
  "x!!",
  "getshaky",
  "rockthebox",
  "rockthabox",
  "spacer",
  "learningtofly",
  "bpm",
  "equalizer",
  "2fa",
  "otp",
];
