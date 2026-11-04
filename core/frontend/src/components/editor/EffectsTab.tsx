import { useState } from "react";
import { Plus, Trash2 } from "lucide-react";
import type { RecordEditDoc, RecordEffectValue } from "../../lib/ipc";
import { EFFECTS, effectSpec, effectValues, type EffectParam } from "../../lib/effects";
import { deleteEffect, setEffectValue } from "../../lib/editor";
import { useT, type MessageKey } from "../../lib/i18n";
import { ColorRow, Group, RangeSlider, Toggle, chipClass } from "./InspectorParts";

/** Name and one-line description per catalogue effect (ids from params.json; a test pins that
 *  every catalogue id has both). */
export const EFFECT_TEXT: Record<string, { name: MessageKey; desc: MessageKey }> = {
  vignette: { name: "record.editor.effects.vignette.name", desc: "record.editor.effects.vignette.desc" },
  grain: { name: "record.editor.effects.grain.name", desc: "record.editor.effects.grain.desc" },
  chromatic: { name: "record.editor.effects.chromatic.name", desc: "record.editor.effects.chromatic.desc" },
  ripple: { name: "record.editor.effects.ripple.name", desc: "record.editor.effects.ripple.desc" },
  spotlight: { name: "record.editor.effects.spotlight.name", desc: "record.editor.effects.spotlight.desc" },
  scanlines: { name: "record.editor.effects.scanlines.name", desc: "record.editor.effects.scanlines.desc" },
};

/** Label per parameter name (shared across effects: `radius` means the same everywhere). */
export const PARAM_LABEL: Record<string, MessageKey> = {
  strength: "record.editor.effects.param.strength",
  radius: "record.editor.effects.param.radius",
  softness: "record.editor.effects.param.softness",
  color: "record.editor.effects.param.color",
  amount: "record.editor.effects.param.amount",
  size: "record.editor.effects.param.size",
  animated: "record.editor.effects.param.animated",
  center_clear: "record.editor.effects.param.center_clear",
  duration: "record.editor.effects.param.duration",
  shine: "record.editor.effects.param.shine",
  dim: "record.editor.effects.param.dim",
  intensity: "record.editor.effects.param.intensity",
  spacing: "record.editor.effects.param.spacing",
  roll: "record.editor.effects.param.roll",
};

/** Units the catalogue documents (README: `size`/`spacing` in 1080p px, `duration` in s). */
const UNIT: Record<string, string> = { size: " px", spacing: " px", duration: " s" };

const fmtParam = (name: string, v: number) => `${Math.round(v * 100) / 100}${UNIT[name] ?? ""}`;

export function effectName(t: (k: MessageKey) => string, id: string): string {
  const k = EFFECT_TEXT[id]?.name;
  return k ? t(k) : id;
}

export interface EffectsTabProps {
  doc: RecordEditDoc;
  /** The effect region (`id`) selected on the timeline. */
  selected: string | null;
  onSelect: (id: string | null) => void;
  /** A new region of effect `id` at the playhead (the editor maps it through Rust). */
  onAdd: (effectId: string) => void;
  onCommit: (doc: RecordEditDoc) => void;
  onLive: (doc: RecordEditDoc) => void;
  onLiveEnd: () => void;
  /** Whether a clip has recorded pointer data (ripple/spotlight need it). */
  clipHasPointer: (clip: number) => boolean;
}

/**
 * Built-in shader effects: an add list (name + one line each), then the selected region's
 * controls — one per declared parameter. Values move live (sliders, colour) or as one step
 * (toggles); delete asks once inline.
 */
export function EffectsTab({ doc, selected, onSelect, onAdd, onCommit, onLive, onLiveEnd, clipHasPointer }: EffectsTabProps) {
  const t = useT();
  const [menuOpen, setMenuOpen] = useState(false);
  const [armed, setArmed] = useState<string | null>(null);
  const r = selected ? (doc.effects ?? []).find((x) => x.id === selected) : undefined;
  const spec = r ? effectSpec(r.effect) : undefined;
  // A different selection disarms a pending delete (derived during render).
  const [seen, setSeen] = useState(selected);
  if (seen !== selected) {
    setSeen(selected);
    setArmed(null);
  }
  const values = r ? effectValues(r.effect, r.values) : {};
  const set = (live: boolean, name: string, v: RecordEffectValue) => {
    if (!r) return;
    const next = setEffectValue(doc, r.id, name, v);
    if (live) onLive(next);
    else onCommit(next);
  };
  const control = (p: EffectParam) => {
    const label = PARAM_LABEL[p.name] ? t(PARAM_LABEL[p.name]) : p.name;
    const v = values[p.name];
    if (p.type === "number") {
      return (
        <RangeSlider
          key={p.name}
          label={label}
          min={p.min}
          max={p.max}
          step={(p.max - p.min) / 100}
          value={typeof v === "number" ? v : p.default}
          format={(x) => fmtParam(p.name, x)}
          onLive={(x) => set(true, p.name, x)}
          onEnd={onLiveEnd}
        />
      );
    }
    if (p.type === "toggle") {
      return <Toggle key={p.name} label={label} checked={typeof v === "boolean" ? v : p.default} onChange={(x) => set(false, p.name, x)} />;
    }
    return (
      <ColorRow
        key={p.name}
        label={label}
        value={typeof v === "string" ? v : p.default}
        onLive={(x) => set(true, p.name, x)}
        onEnd={onLiveEnd}
      />
    );
  };
  return (
    <div className="space-y-4" data-testid="ed-effects-tab">
      <Group label={t("record.editor.effects.tab")} hint={t("record.editor.effects.add_hint")}>
        <button
          type="button"
          aria-expanded={menuOpen}
          onClick={() => setMenuOpen((o) => !o)}
          className={chipClass(menuOpen) + " inline-flex items-center gap-1"}
        >
          <Plus size={12} /> {t("record.editor.effects.add")}
        </button>
        {menuOpen && (
          <ul role="menu" aria-label={t("record.editor.effects.add")} className="space-y-1" data-testid="ed-effects-menu">
            {EFFECTS.map((e) => (
              <li key={e.id} role="none">
                <button
                  type="button"
                  role="menuitem"
                  onClick={() => {
                    setMenuOpen(false);
                    onAdd(e.id);
                  }}
                  className="w-full rounded-md border border-[var(--color-border)] px-2 py-1.5 text-left transition-colors duration-(--duration-fast) ease-sharp hover:bg-[var(--color-bg)]"
                >
                  <span className="block text-[12px]">{effectName(t, e.id)}</span>
                  {EFFECT_TEXT[e.id] && <span className="block text-[11px] text-[var(--color-muted)]">{t(EFFECT_TEXT[e.id].desc)}</span>}
                </button>
              </li>
            ))}
          </ul>
        )}
      </Group>
      {!r || !spec ? (
        <p className="text-[12px] text-[var(--color-muted)]" data-testid="ed-effects-none">
          {t("record.editor.effects.none_selected")}
        </p>
      ) : (
        <Group label={effectName(t, r.effect)} hint={EFFECT_TEXT[r.effect] ? t(EFFECT_TEXT[r.effect].desc) : undefined}>
          {spec.needsPointer && !clipHasPointer(r.clip) && (
            <p className="text-[11px] text-amber-500" data-testid="ed-effect-no-pointer">
              {t("record.editor.effects.needs_pointer")}
            </p>
          )}
          <div className="space-y-2">{spec.params.map(control)}</div>
          {armed === r.id ? (
            <div className="flex flex-wrap items-center gap-1" data-testid="ed-effect-confirm">
              <span className="text-[12px]">{t("record.editor.effects.delete_ask")}</span>
              <button
                type="button"
                className={chipClass(false) + " text-red-500"}
                onClick={() => {
                  setArmed(null);
                  onCommit(deleteEffect(doc, r.id));
                  onSelect(null);
                }}
              >
                {t("record.editor.effects.delete_confirm")}
              </button>
              <button type="button" className={chipClass(false)} onClick={() => setArmed(null)}>
                {t("record.editor.effects.delete_cancel")}
              </button>
            </div>
          ) : (
            <button type="button" className={chipClass(false) + " inline-flex items-center gap-1 text-red-500"} onClick={() => setArmed(r.id)}>
              <Trash2 size={12} /> {t("record.editor.effects.delete")}
            </button>
          )}
        </Group>
      )}
    </div>
  );
}
