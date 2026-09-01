import { useEffect, useState } from "react";
import {
  chipNeedsDark,
  iconForIssuer,
  loadIconIndex,
  monogramHue,
  monogramInitial,
  peekIconIndex,
  type IconIndex,
} from "../lib/totp-icons";

/**
 * Brand icon chip for a 2FA issuer (v0.161.0; shared v0.162.0) — ONE
 * implementation for the 2fa overlay rows, the `otp <issuer>` main-list rows
 * and the TOTP preview, so the three surfaces can never drift.
 *
 * Self-loading: mounting triggers the memoised lazy import of the 4.5 MB
 * Simple Icons chunk — so the chunk still stays out of the startup path (it
 * only loads once a TOTP surface actually renders), and `peekIconIndex`
 * seeds already-loaded data synchronously so remounts never flash the
 * monogram first.
 *
 * The chip is deliberately LIGHT in both themes (the icon-catalogue lesson:
 * GitHub #181717 vanishes on a dark ground); near-white brands flip via
 * `chipNeedsDark`. `fill` sits on <svg> AND <path> as presentation
 * attributes. An unknown issuer gets a deterministic monogram — never a
 * guessed look-alike brand.
 */
export function TotpBrandIcon({ issuer, size = 28 }: { issuer: string; size?: number }) {
  const [icons, setIcons] = useState<IconIndex | null>(peekIconIndex());
  useEffect(() => {
    if (icons) return;
    let live = true;
    loadIconIndex()
      .then((ix) => {
        if (live) setIcons(ix);
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [icons]);

  const icon = icons ? iconForIssuer(icons, issuer) : null;
  const box = { width: size, height: size };
  const glyph = Math.round(size * 0.61);
  const base = "flex shrink-0 items-center justify-center rounded-md border ";
  if (icon) {
    const dark = chipNeedsDark(icon.hex);
    return (
      <span
        title={icon.title}
        style={box}
        className={base + (dark ? "border-[#3a4150] bg-[#1c212b]" : "border-[#e4e7ee] bg-[#f1f3f7]")}
      >
        <svg
          viewBox="0 0 24 24"
          width={glyph}
          height={glyph}
          role="img"
          aria-label={icon.title}
          fill={"#" + icon.hex}
        >
          <path fill={"#" + icon.hex} d={icon.path} />
        </svg>
      </span>
    );
  }
  return (
    <span
      aria-hidden="true"
      style={{ ...box, color: `hsl(${monogramHue(issuer)} 55% 38%)`, fontSize: Math.max(10, Math.round(size * 0.43)) }}
      className={base + "border-[#e4e7ee] bg-[#f1f3f7] font-bold"}
    >
      {monogramInitial(issuer)}
    </span>
  );
}
