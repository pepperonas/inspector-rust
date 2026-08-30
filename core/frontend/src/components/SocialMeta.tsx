import { useSyncExternalStore } from "react";
import { socialMetadata } from "../lib/ipc";
import { createMetaLoader, formatDuration, clampDescription } from "../lib/social-meta";

/**
 * ONE loader for the whole app, so the single-link preview and the link
 * grabber share a cache: the seed link appears in both, and metadata costs
 * ~4 s per URL. The cap keeps a pasted list of thirty from spawning thirty
 * yt-dlp processes at once.
 */
const loader = createMetaLoader(socialMetadata, 3);

/** Subscribe one URL's metadata; queues the fetch on first use.
 *  Exported so the download bar can read the duration for the trim bar out of
 *  the SAME cache MetaCard fills -- no second yt-dlp call. */
export function useMeta(url: string | null) {
  const state = useSyncExternalStore(
    (fn) => loader.subscribe(fn),
    () => (url ? loader.get(url) : undefined),
  );
  if (url && !state) loader.request([url]);
  return state;
}

/** Queue a whole list at once (the grabber). */
export function requestMeta(urls: readonly string[]) {
  loader.request(urls);
}

const Thumb = ({ src, w, h }: { src: string | null | undefined; w: number; h: number }) =>
  src ? (
    // The app has no CSP, so a remote thumbnail loads directly — same as the
    // Shazam cover art. A dead URL just leaves the placeholder.
    <img
      src={src}
      alt=""
      loading="lazy"
      width={w}
      height={h}
      className="shrink-0 rounded object-cover"
      style={{ width: w, height: h, background: "var(--color-surface)" }}
    />
  ) : (
    <div
      className="shrink-0 rounded border border-[var(--color-border)]"
      style={{ width: w, height: h, background: "var(--color-surface)" }}
    />
  );

/** The rich card under the single-link download bar. */
export function MetaCard({ url }: { url: string }) {
  const s = useMeta(url);
  if (!s || s.state === "failed") return null; // a failure is not worth a banner here
  if (s.state === "loading") {
    return (
      <div className="mt-3 flex items-center gap-3">
        <div className="h-[54px] w-[96px] shrink-0 animate-pulse rounded bg-[var(--color-surface)]" />
        <div className="flex min-w-0 flex-1 flex-col gap-1.5">
          <div className="h-3 w-3/4 animate-pulse rounded bg-[var(--color-surface)]" />
          <div className="h-2.5 w-1/2 animate-pulse rounded bg-[var(--color-surface)]" />
        </div>
      </div>
    );
  }
  const m = s.meta;
  const dur = formatDuration(m.duration_s);
  return (
    <div className="mt-3 flex flex-col gap-2">
      <div className="flex items-start gap-3">
        <Thumb src={m.thumbnail} w={96} h={54} />
        <div className="min-w-0 flex-1">
          <p className="line-clamp-2 text-[12px] font-medium leading-4">{m.title}</p>
          <p className="mt-0.5 text-[11px] text-[var(--color-muted)]">
            {[m.uploader, dur].filter(Boolean).join(" · ")}
          </p>
        </div>
      </div>
      {m.description && (
        <p className="text-[11px] leading-4 text-[var(--color-muted)]">
          {clampDescription(m.description, 260)}
        </p>
      )}
    </div>
  );
}

/** The compact form for one row of the grabber's list. */
export function MetaRow({ url }: { url: string }) {
  const s = useMeta(url);
  if (s?.state === "ok") {
    const dur = formatDuration(s.meta.duration_s);
    return (
      <>
        <Thumb src={s.meta.thumbnail} w={40} h={23} />
        <span className="min-w-0 flex-1 truncate" title={s.meta.title}>
          {s.meta.title}
        </span>
        {dur && <span className="shrink-0 text-[10px] text-[var(--color-muted)]">{dur}</span>}
      </>
    );
  }
  return (
    <>
      <Thumb src={null} w={40} h={23} />
      <span className="min-w-0 flex-1 truncate font-[var(--font-mono)] text-[var(--color-muted)]" title={url}>
        {s?.state === "loading" ? url : url}
      </span>
    </>
  );
}
