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
