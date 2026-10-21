/**
 * Which `shazam` view a command line asks for. Pure.
 *
 * - `shazam` → listen (identify the song)
 * - `shazam history` → the recognized-songs list
 * - `shazam lyrics` / `shazam songtext` / the `lyrics` keyword → identify,
 *   then fetch + copy the lyrics
 */
export type ShazamView = "listen" | "history" | "lyrics";

export function shazamView(arg?: string | null, keyword?: string | null): ShazamView {
  if (keyword === "lyrics") return "lyrics";
  const a = arg ?? "";
  if (/\b(lyric|songtext|text)/i.test(a)) return "lyrics";
  if (/\b(hist|list)/i.test(a)) return "history";
  return "listen";
}

/** The three views as discoverable sub-rows (the OTHER two are shown). */
export const SHAZAM_VIEWS: ReadonlyArray<{
  view: ShazamView;
  completion: string;
  descKey: "cmd.shazam.sub_identify" | "cmd.shazam.sub_history" | "cmd.shazam.sub_lyrics";
}> = [
  { view: "listen", completion: "shazam", descKey: "cmd.shazam.sub_identify" },
  { view: "lyrics", completion: "shazam lyrics", descKey: "cmd.shazam.sub_lyrics" },
  { view: "history", completion: "shazam history", descKey: "cmd.shazam.sub_history" },
];
