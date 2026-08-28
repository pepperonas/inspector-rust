/**
 * Detect a downloadable social-media URL (YouTube / Instagram / TikTok /
 * Facebook / Dailymotion) in a string — a clip's text or the search query. Pure; mirrors the
 * Rust `social_dl::detect_platform`. Used to surface a download suggestion in
 * the preview.
 */

export type SocialPlatform =
  | "youtube"
  | "instagram"
  | "tiktok"
  | "facebook"
  | "dailymotion";

export interface SocialTarget {
  platform: SocialPlatform;
  url: string;
}

const LABELS: Record<SocialPlatform, string> = {
  youtube: "YouTube",
  instagram: "Instagram",
  tiktok: "TikTok",
  facebook: "Facebook",
  dailymotion: "Dailymotion",
};

export function platformLabel(p: SocialPlatform): string {
  return LABELS[p];
}

/** Find the first http(s) URL in `text` and classify it; null if not social. */
export function detectSocial(text: string): SocialTarget | null {
  const m = text.match(/https?:\/\/[^\s<>"']+/i);
  if (!m) return null;
  const url = m[0];
  const u = url.toLowerCase();
  if (u.includes("youtube.com") || u.includes("youtu.be")) return { platform: "youtube", url };
  if (u.includes("instagram.com")) return { platform: "instagram", url };
  if (u.includes("tiktok.com")) return { platform: "tiktok", url };
  if (u.includes("facebook.com") || u.includes("fb.watch") || u.includes("fb.com"))
    return { platform: "facebook", url };
  // `dai.ly` is Dailymotion's own short form.
  if (u.includes("dailymotion.com") || u.includes("dai.ly"))
    return { platform: "dailymotion", url };
  return null;
}

/**
 * Every social URL in arbitrary pasted text, in order, deduplicated.
 *
 * The point of the link grabber is that you paste whatever you have — a list,
 * a chat log, an e-mail, half a web page — so this is deliberately forgiving
 * about what surrounds a link, and deliberately strict about where one ends.
 *
 * ⚠️ **Trailing punctuation is the whole difficulty.** Prose puts a period
 * after a link, brackets around it, and a comma between two of them; a URL
 * that keeps them is a 404. Closing brackets are only stripped when they are
 * *unbalanced* — a link may legitimately contain `(` … `)` — which is the
 * same rule linkifiers use, for the same reason.
 */
export function extractSocialLinks(text: string): SocialTarget[] {
  const out: SocialTarget[] = [];
  const seen = new Set<string>();
  for (const m of text.matchAll(/https?:\/\/[^\s<>"'`]+/gi)) {
    const url = trimUrlTail(m[0]);
    if (!url || seen.has(url)) continue;
    const hit = detectSocial(url);
    // `detectSocial` scans for the FIRST url in its input; here the input is
    // already exactly one, so a hit classifies this link and nothing else.
    if (!hit) continue;
    seen.add(url);
    out.push(hit);
  }
  return out;
}

/** Strip sentence punctuation a paste dragged in, leaving balanced brackets. */
function trimUrlTail(raw: string): string {
  let s = raw;
  for (;;) {
    // `s.at(-1)` would need lib ES2022; the tsconfig targets ES2020.
    const last: string | undefined = s[s.length - 1];
    if (!last) return s;
    if (".,;:!?".includes(last)) {
      s = s.slice(0, -1);
      continue;
    }
    const pairs: Record<string, string | undefined> = { ")": "(", "]": "[", "}": "{" };
    const open = pairs[last];
    if (open) {
      const opens = s.split(open).length - 1;
      const closes = s.split(last).length - 1;
      if (closes > opens) {
        s = s.slice(0, -1);
        continue;
      }
    }
    return s;
  }
}

/** True when every target is YouTube — the only platform the UI offers audio for. */
export function allYouTube(targets: readonly SocialTarget[]): boolean {
  return targets.length > 0 && targets.every((t) => t.platform === "youtube");
}
