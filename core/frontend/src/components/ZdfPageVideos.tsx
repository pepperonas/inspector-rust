import { useEffect, useRef, useState } from "react";
import { Check, Download, Film, Loader2, X } from "lucide-react";
import { revealPath, setSuppressHide, socialDownload, zdfPageVideos, type PageVideo } from "../lib/ipc";
import { useMeta } from "./SocialMeta";
import { formatDuration } from "../lib/social-meta";
import { t, tn, useT } from "../lib/i18n";

type RowState = "idle" | "running" | "done" | "failed";

/**
 * Every video on a zdfheute article.
 *
 * yt-dlp's `zdf` extractor only returns the lead player of an article, but
 * articles often embed more (an interview, a live segment, a teaser). Rust
 * reads them out of the page (`zdf_page_videos`); each row downloads on its
 * own, "all" runs them one after another. Hidden when the page has at most
 * one video — the download bar above already covers that one.
 *
 * Same batch rules as the link grabber: sequential, a failure does not stop
 * the run, the popup is pinned during the run and Finder opens once at the end.
 */
export function ZdfPageVideos({ url }: { url: string }) {
  useT();
  const [videos, setVideos] = useState<PageVideo[] | null>(null);
  const [state, setState] = useState<Record<string, { s: RowState; note?: string }>>({});
  const [running, setRunning] = useState(false);
  const stopRef = useRef(false);

  useEffect(() => {
    let alive = true;
    setVideos(null);
    setState({});
    zdfPageVideos(url)
      .then((v) => alive && setVideos(v))
      .catch(() => alive && setVideos([]));
    return () => {
      alive = false;
    };
  }, [url]);

  useEffect(() => () => void setSuppressHide(false).catch(() => undefined), []);

  const run = async (list: PageVideo[]) => {
    if (running || list.length === 0) return;
    stopRef.current = false;
    setRunning(true);
    void setSuppressHide(true).catch(() => undefined);
    let lastOk: string | null = null;
    for (const v of list) {
      if (stopRef.current) break;
      setState((s) => ({ ...s, [v.id]: { s: "running" } }));
      try {
        const out = await socialDownload(v.url, "video", false, undefined, v.title);
        lastOk = out;
        setState((s) => ({ ...s, [v.id]: { s: "done", note: out.split(/[/\\]/).pop() || out } }));
      } catch (e) {
        const msg = String(e).replace(/^Error:\s*/, "");
        setState((s) => ({ ...s, [v.id]: { s: "failed", note: msg } }));
      }
    }
    setRunning(false);
    void setSuppressHide(false).catch(() => undefined);
    if (lastOk) void revealPath(lastOk).catch(() => undefined);
  };

  if (videos === null) {
    return (
      <div className="mt-3 flex items-center gap-2 text-[11px] text-[var(--color-muted)]">
        <Loader2 size={11} className="animate-spin" /> {t("zdf.scanning")}
      </div>
    );
  }
  if (videos.length < 2) return null;

  return (
    <div className="mt-4 flex flex-col gap-2 border-t border-[var(--color-border)] pt-3" data-testid="zdf-page-videos">
      <div className="flex items-center gap-2 text-[11px]">
        <Film size={12} className="text-[var(--color-muted)]" />
        <span className="uppercase tracking-wide text-[var(--color-muted)]">{tn("zdf.found", videos.length)}</span>
        <div className="ml-auto">
          {running ? (
            <button
              type="button"
              onClick={() => {
                stopRef.current = true;
              }}
              className="md3-press rounded border border-[var(--color-border)] px-3 py-1 text-[11px] hover:border-rose-400 hover:text-rose-400"
            >
              <X size={11} className="inline" /> {t("lg.stop_after")}
            </button>
          ) : (
            <button
              type="button"
              onClick={() => void run(videos)}
              className="md3-press rounded bg-[var(--color-accent)] px-3 py-1 text-[11px] font-medium text-[var(--color-accent-fg)]"
            >
              <Download size={11} className="inline" /> {tn("zdf.download_all", videos.length)}
            </button>
          )}
        </div>
      </div>
      <ul className="flex flex-col gap-1">
        {videos.map((v) => (
          <VideoRow
            key={v.id}
            video={v}
            state={state[v.id]?.s ?? "idle"}
            note={state[v.id]?.note}
            disabled={running}
            onDownload={() => void run([v])}
          />
        ))}
      </ul>
    </div>
  );
}

function VideoRow({
  video,
  state,
  note,
  disabled,
  onDownload,
}: {
  video: PageVideo;
  state: RowState;
  note?: string;
  disabled: boolean;
  onDownload: () => void;
}) {
  // Only the duration comes from yt-dlp: its title for a bare stream would be
  // the playlist's file name, the page knows the real one.
  const meta = useMeta(video.url);
  const dur = meta?.state === "ok" ? formatDuration(meta.meta.duration_s) : null;
  return (
    <li className="flex items-start gap-2 rounded px-1 py-1 text-[11px]">
      {video.thumbnail ? (
        <img
          src={video.thumbnail}
          alt=""
          loading="lazy"
          className="h-[36px] w-[64px] shrink-0 rounded object-cover"
          style={{ background: "var(--color-surface)" }}
        />
      ) : (
        <div className="h-[36px] w-[64px] shrink-0 rounded border border-[var(--color-border)]" />
      )}
      <div className="min-w-0 flex-1">
        <p className="truncate font-medium" title={video.title}>
          {video.title}
          {dur && <span className="ml-1.5 font-normal text-[var(--color-muted)]">{dur}</span>}
        </p>
        <p
          className={
            "line-clamp-2 leading-4 " + (state === "failed" ? "text-rose-400" : "text-[var(--color-muted)]")
          }
          title={note ?? video.description}
        >
          {note ?? video.description}
        </p>
      </div>
      <div className="flex h-[36px] shrink-0 items-center">
        {state === "running" ? (
          <Loader2 size={13} className="animate-spin text-[var(--color-accent)]" />
        ) : state === "done" ? (
          <Check size={13} className="text-emerald-400" />
        ) : (
          <button
            type="button"
            disabled={disabled}
            onClick={onDownload}
            title={state === "failed" ? t("zdf.retry") : t("zdf.download_one")}
            className="md3-press rounded border border-[var(--color-border)] p-1 hover:border-[var(--color-accent)] disabled:opacity-40"
          >
            {state === "failed" ? <X size={12} className="text-rose-400" /> : <Download size={12} />}
          </button>
        )}
      </div>
    </li>
  );
}
