import { useEffect, useMemo, useRef, useState } from "react";
import { Check, Download, ListPlus, Loader2, Music, Trash2, X } from "lucide-react";
import { extractSocialLinks, allYouTube, platformLabel, type SocialTarget } from "../lib/social";
import { revealPath, setSuppressHide, socialDownload } from "../lib/ipc";

type ItemState = "pending" | "running" | "done" | "failed";

interface QueueItem {
  url: string;
  target: SocialTarget;
  state: ItemState;
  note?: string;
}

/**
 * Paste arbitrarily many links, download all of them.
 *
 * Sits under the single-link download + status section. The extraction is
 * deliberately forgiving (`extractSocialLinks` — pure and unit-tested): paste
 * a list, a chat log, an e-mail, and every social link in it is picked out,
 * deduplicated, in order.
 *
 * Three things a batch needs that a single download does not:
 *
 *  * **Sequential, not parallel.** yt-dlp saturates a link on its own, and the
 *    cookies-from-browser retry would have several processes racing for the
 *    same keychain prompt.
 *  * **A failure must not stop the queue.** One dead link in twelve is normal;
 *    it is recorded on its row and the run continues.
 *  * **The popup must stay open.** It hides on focus loss, which would unmount
 *    this component and abandon the run — so the batch pins it
 *    (`setSuppressHide`) and downloads with `reveal: false`, revealing once at
 *    the end instead of raising Finder after every file.
 */
export function LinkGrabber({ seedUrl }: { seedUrl?: string }) {
  const [text, setText] = useState("");
  const [queue, setQueue] = useState<QueueItem[] | null>(null);
  const [mode, setMode] = useState<"video" | "audio">("video");
  const [running, setRunning] = useState(false);
  const stopRef = useRef(false);

  const found = useMemo(() => extractSocialLinks(text), [text]);
  // Audio only when EVERY link is YouTube — the same promise the single-link
  // bar above makes. yt-dlp would extract audio elsewhere too, but offering it
  // here and not there would be two different answers to one question.
  const audioOffered = allYouTube(found);
  useEffect(() => {
    if (!audioOffered && mode === "audio") setMode("video");
  }, [audioOffered, mode]);

  // Releasing the pin on unmount matters more than it looks: without it an
  // interrupted run would leave the popup unable to close.
  useEffect(() => () => void setSuppressHide(false).catch(() => undefined), []);

  const run = async (targets: SocialTarget[]) => {
    if (running || targets.length === 0) return;
    stopRef.current = false;
    setRunning(true);
    void setSuppressHide(true).catch(() => undefined);
    const items: QueueItem[] = targets.map((t) => ({ url: t.url, target: t, state: "pending" }));
    setQueue(items);
    let lastOk: string | null = null;

    for (let i = 0; i < items.length; i++) {
      if (stopRef.current) break;
      setQueue((q) => q && q.map((it, j) => (j === i ? { ...it, state: "running" } : it)));
      try {
        const out = await socialDownload(items[i].url, mode, false);
        lastOk = out;
        const name = out.split(/[/\\]/).pop() || out;
        setQueue((q) => q && q.map((it, j) => (j === i ? { ...it, state: "done", note: name } : it)));
      } catch (e) {
        const msg = String(e);
        const note = msg.includes("no_ytdlp")
          ? "yt-dlp not installed — brew install yt-dlp"
          : msg.replace(/^Error:\s*/, "");
        setQueue((q) => q && q.map((it, j) => (j === i ? { ...it, state: "failed", note } : it)));
      }
    }

    setRunning(false);
    void setSuppressHide(false).catch(() => undefined);
    if (lastOk) void revealPath(lastOk).catch(() => undefined);
  };

  const failed = queue?.filter((i) => i.state === "failed") ?? [];
  const doneCount = queue?.filter((i) => i.state === "done").length ?? 0;

  return (
    <div className="mt-4 flex flex-col gap-2 border-t border-[var(--color-border)] pt-3">
      <div className="flex items-center gap-2 text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
        <ListPlus size={12} />
        <span>Link grabber</span>
        {seedUrl && <span className="normal-case tracking-normal">— paste more links to batch-download</span>}
      </div>

      <textarea
        value={text}
        onChange={(e) => setText(e.target.value)}
        disabled={running}
        rows={4}
        spellCheck={false}
        placeholder={"Paste anything — a list, a chat log, an e-mail.\nEvery YouTube / Instagram / TikTok / Facebook / Dailymotion link in it is picked out."}
        className="w-full resize-y rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-2 font-[var(--font-mono)] text-[11px] leading-5 outline-none placeholder:text-[var(--color-muted)] focus:border-[var(--color-accent)] disabled:opacity-50"
      />

      <div className="flex flex-wrap items-center gap-2 text-[11px]">
        <span className="text-[var(--color-muted)]">
          {found.length === 0
            ? "No links yet"
            : `${found.length} link${found.length === 1 ? "" : "s"} found`}
        </span>
        {audioOffered && (
          <div className="flex items-center gap-1">
            {(["video", "audio"] as const).map((m) => (
              <button
                key={m}
                type="button"
                disabled={running}
                onClick={() => setMode(m)}
                className={
                  "rounded-full border px-2 py-0.5 uppercase tracking-wide disabled:opacity-40 " +
                  (mode === m
                    ? "border-[var(--color-accent)] text-[var(--color-accent)]"
                    : "border-[var(--color-border)] text-[var(--color-muted)]")
                }
              >
                {m === "audio" ? <Music size={10} className="inline" /> : <Download size={10} className="inline" />} {m}
              </button>
            ))}
          </div>
        )}
        <div className="ml-auto flex items-center gap-1.5">
          {text && !running && (
            <button
              type="button"
              onClick={() => {
                setText("");
                setQueue(null);
              }}
              title="Clear"
              className="rounded-md p-1 text-[var(--color-muted)] hover:text-[var(--color-fg)]"
            >
              <Trash2 size={12} />
            </button>
          )}
          {running ? (
            <button
              type="button"
              onClick={() => {
                stopRef.current = true;
              }}
              className="md3-press rounded border border-[var(--color-border)] px-3 py-1 text-[11px] hover:border-rose-400 hover:text-rose-400"
            >
              <X size={11} className="inline" /> Stop after current
            </button>
          ) : (
            <button
              type="button"
              disabled={found.length === 0}
              onClick={() => void run(found)}
              className="md3-press rounded bg-[var(--color-accent)] px-3 py-1 text-[11px] font-medium text-[var(--color-accent-fg)] disabled:opacity-40"
            >
              <Download size={11} className="inline" /> Download {found.length || ""} {found.length === 1 ? "link" : "links"}
            </button>
          )}
        </div>
      </div>

      {queue && (
        <>
          <div className="text-[11px] text-[var(--color-muted)]">
            {doneCount} / {queue.length} done{failed.length > 0 && ` · ${failed.length} failed`}
          </div>
          <ul className="flex max-h-56 flex-col gap-0.5 overflow-y-auto">
            {queue.map((it, i) => (
              <li key={`${it.url}-${i}`} className="flex items-center gap-2 rounded px-1 py-1 text-[11px]">
                <StateIcon state={it.state} />
                <span className="w-[68px] shrink-0 text-[10px] uppercase tracking-wide text-[var(--color-muted)]">
                  {platformLabel(it.target.platform)}
                </span>
                <span className="min-w-0 flex-1 truncate font-[var(--font-mono)] text-[var(--color-muted)]" title={it.url}>
                  {it.note ?? it.url}
                </span>
              </li>
            ))}
          </ul>
          {failed.length > 0 && !running && (
            <button
              type="button"
              onClick={() => void run(failed.map((f) => f.target))}
              className="md3-press self-start rounded border border-[var(--color-border)] px-3 py-1 text-[11px] hover:border-[var(--color-accent)]"
            >
              Retry {failed.length} failed
            </button>
          )}
        </>
      )}
    </div>
  );
}

function StateIcon({ state }: { state: ItemState }) {
  if (state === "done") return <Check size={12} className="shrink-0 text-emerald-400" />;
  if (state === "failed") return <X size={12} className="shrink-0 text-rose-400" />;
  if (state === "running") return <Loader2 size={12} className="shrink-0 animate-spin text-[var(--color-accent)]" />;
  return <span className="h-[12px] w-[12px] shrink-0 rounded-full border border-[var(--color-border)]" />;
}
