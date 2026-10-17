# Social video downloads

Paste a supported link into the search bar or select a clipboard entry
containing one. Inspector offers a download row and a video download button
in the preview. Downloads use the installed `yt-dlp`, with ffmpeg for muxing
or extraction, and go to Downloads. X/Twitter support was added in v0.215.2.

## Supported platforms and links

| Platform | Hosts | Modes offered |
|---|---|---|
| YouTube | `youtube.com`, `youtu.be`, including subdomains | Video and audio |
| Instagram | `instagram.com`, including subdomains | Video |
| TikTok | `tiktok.com`, including short-link subdomains | Video |
| Facebook | `facebook.com`, `fb.com`, `fb.watch` and subdomains | Video |
| Dailymotion | `dailymotion.com`, `dai.ly` and subdomains | Video |
| X / Twitter | `x.com`, `twitter.com`, including `www` and mobile subdomains | Video |

Links need an HTTP or HTTPS scheme. The original link, including status ID
and query parameters, is passed to the downloader. Legacy Twitter links do
not need to be rewritten to X links first. Recognition uses the actual
hostname: `x.com.example.org`, `notx.com` or a different site's
`?next=https://x.com/…` parameter is not an X download.

For example, all of these are recognised:

```text
https://x.com/elonmusk/status/2110923689797808453
https://www.x.com/elonmusk/status/2110923689797808453?s=20
https://twitter.com/elonmusk/status/2110923689797808453
https://mobile.twitter.com/elonmusk/status/2110923689797808453
https://x.com/i/status/2110923689797808453
```

## Search bar, clipboard and QR actions

For a pasted X/Twitter link, the social download row is labelled **X** and
appears before the existing QR action. Select it to see **Download video**;
Enter runs the selected video download. QR generation remains separately
available for the same URL. A clipboard text entry containing an X link
offers the same download controls in its preview.

The detector inspects the first HTTP(S) URL in ordinary text. If a different,
unsupported link precedes the video link, paste the video URL by itself or
use the link grabber, which scans all links.

The separate audio button remains limited to YouTube. X videos use the
existing video path, which prefers H.264 and an MP4 container when the
available formats allow it. Video files retain their soundtrack.

## Mixed batch downloads

Use the link grabber below the download bar to paste a list, chat log or
other text. It extracts supported links, removes duplicate URLs, strips
surrounding sentence punctuation and downloads sequentially. X, legacy
Twitter and other supported platforms can be mixed in one queue. A failed
link does not stop later entries; Retry runs failed entries again. Audio is
offered only when every queued link is YouTube.

Deduplication compares URL strings, so an X URL and a legacy Twitter URL for
the same post remain distinct entries. Metadata and thumbnails share the
existing cache and request limits. See [CLAUDE.md](../CLAUDE.md) for the
provider/queue architecture and [features.txt](../features.txt) for the
feature catalogue.

## Troubleshooting and verification

If only QR generation appears for a supported X URL, check that the running
app is v0.215.2 or later. Older versions omitted X from both platform
detectors, even though yt-dlp could already extract the video.

Recognition classifies the host; it does not establish that every post has
downloadable media. Posts without video, deleted/private content, login
requirements or service errors are reported by yt-dlp in the download
preview. If yt-dlp is missing, Inspector shows its install hint. No new
account connection or app permission is introduced by this fix.

The reported Elon Musk post was checked with the locally installed yt-dlp:
metadata identified a 48.53-second video and 11 formats. A real download of the same URL produced an MP4 with H.264 video and AAC audio; ffprobe reported 48.576 seconds and 11,332,902 bytes. The smoke-test file was written to a temporary directory, not the user's Downloads folder. Frontend and Rust
regression tests include the exact URL, legacy/mobile variants and negative
hostname cases. UI tests verify the X video button and original URL/mode
sent to IPC; queue tests verify mixed X/Twitter/YouTube batches. The network
probe is a manual check, not part of the deterministic unit tests.
