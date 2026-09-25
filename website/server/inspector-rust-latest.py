#!/usr/bin/env python3
"""Publish the newest Inspector Rust release next to its product page.

Generated from templates/apps/product-page. Runs on the VPS from inspector-rust-latest.timer (every 15 min)
and writes, each file only when its content changed:

  <webroot>/latest.json      release facts for the page's JavaScript and for agents (same origin: visitors
                             never call GitHub themselves)
  <webroot>/ssi/*            the same facts as tiny fragments, pulled into index.html / index.md by nginx
                             SSI — so they are in the document without JavaScript
  <webroot>/changelog.md     CHANGELOG.md from the default branch, for the changelog dialog
  /etc/nginx/inspector-rust-download.conf
                             /download/<target> -> 302 to that target's newest asset, and /download ->
                             the target the visitor's browser asks for (nginx map in the vhost)

A failed GitHub call, a release missing one of the configured assets, or an odd changelog changes
nothing: the last good state stays online. nginx is reloaded only when the redirects changed, only after
`nginx -t` passed, and never while certbot is running.
"""
import datetime, html, json, os, re, subprocess, sys, tempfile, urllib.request

CONFIG = json.loads(r'''{
 "repo": "pepperonas/inspector-rust",
 "branch": "main",
 "slug": "inspector-rust",
 "domain": "inspector-rust.celox.io",
 "webroot": "/var/www/inspector-rust.celox.io",
 "nginx_include": "/etc/nginx/inspector-rust-download.conf",
 "nginx_var": "inspector_rust",
 "targets": [
  {
   "id": "macos",
   "label": "Download for macOS",
   "short": "macOS",
   "asset": "^InspectorRust_[0-9][0-9A-Za-z.\\-]*_aarch64\\.dmg$",
   "requirement": "macOS 11+ · Apple silicon"
  },
  {
   "id": "windows",
   "label": "Download for Windows",
   "short": "Windows",
   "asset": "^InspectorRust_[0-9][0-9A-Za-z.\\-]*_x64_en-US\\.msi$",
   "requirement": "Windows 10/11 · x64 · installer"
  },
  {
   "id": "windows-exe",
   "label": "Windows portable (.exe)",
   "short": "Windows .exe",
   "asset": "^inspector-rust\\.exe$",
   "requirement": "Windows 10/11 · x64 · no install",
   "optional": true
  },
  {
   "id": "linux-deb",
   "label": "Download .deb",
   "short": "Linux .deb",
   "asset": "^InspectorRust_[0-9][0-9A-Za-z.\\-]*_amd64\\.deb$",
   "requirement": "Ubuntu 24.04+ / Debian 13 · x86-64"
  },
  {
   "id": "linux-appimage",
   "label": "Download AppImage",
   "short": "AppImage",
   "asset": "^InspectorRust_[0-9][0-9A-Za-z.\\-]*_amd64\\.AppImage$",
   "requirement": "Linux x86-64",
   "optional": true
  }
 ]
}''')

REPO = CONFIG["repo"]
BRANCH = CONFIG["branch"]
WEBROOT = os.environ.get("SITE_WEBROOT", CONFIG["webroot"])
NGINX_INC = os.environ.get("SITE_NGINX_INC", CONFIG["nginx_include"])
URL_OK = re.compile(r"^https://github\.com/" + re.escape(REPO) + r"/releases/download/[^\s;\"'{}]+$")
CHANGELOG_URL = f"https://raw.githubusercontent.com/{REPO}/{BRANCH}/CHANGELOG.md"
CHANGELOG_MAX = 2_000_000


def get(url, accept=None):
    headers = {"User-Agent": f"{CONFIG['slug']}-latest"}
    if accept:
        headers["Accept"] = accept
    with urllib.request.urlopen(urllib.request.Request(url, headers=headers), timeout=20) as r:
        return r.read(CHANGELOG_MAX + 1)


def fetch_release():
    rel = json.loads(get(f"https://api.github.com/repos/{REPO}/releases/latest", "application/vnd.github+json"))
    assets = []
    for t in CONFIG["targets"]:
        pattern = re.compile(t["asset"])
        hits = [a for a in rel.get("assets", []) if pattern.search(a.get("name", ""))]
        if len(hits) != 1:
            if t.get("optional"):
                continue
            raise ValueError(f"target {t['id']}: expected one asset matching {t['asset']!r}, found {len(hits)}")
        a = hits[0]
        url = a["browser_download_url"]
        if not URL_OK.match(url):
            raise ValueError(f"unexpected asset URL: {url!r}")
        digest = a.get("digest") or ""
        assets.append({
            "target": t["id"],
            "label": t["label"],
            "short": t["short"],
            "requirement": t.get("requirement", ""),
            "name": a["name"],
            "url": url,
            "size": a["size"],
            "sha256": digest.split(":", 1)[1] if digest.startswith("sha256:") else "",
        })
    if not assets:
        raise ValueError("no configured asset in the latest release")
    first = assets[0]
    return {
        "version": rel["tag_name"],
        "published": rel.get("published_at", ""),
        "notes": rel.get("html_url", ""),
        "assets": assets,
        # The first target's facts at the top level, for tools that expect a single file.
        "name": first["name"], "url": first["url"], "size": first["size"], "sha256": first["sha256"],
    }


def fetch_changelog():
    body = get(CHANGELOG_URL)
    text = body.decode("utf-8")
    if len(body) > CHANGELOG_MAX or not text.startswith("# Changelog") or "\n## [" not in text:
        raise ValueError("unexpected CHANGELOG.md content")
    return text


def mb(size):
    return f"{size / 1048576:.1f} MB"


def ssi_fragments(d):
    """English; the page's JavaScript localises. Plain text or escaped HTML only."""
    first = d["assets"][0]
    day = d["published"][:10]
    try:
        pretty = datetime.date.fromisoformat(day).strftime("%-d %b %Y")
    except ValueError:
        pretty = day
    meta = " · ".join(filter(None, [d["version"], mb(first["size"]), pretty, first["requirement"]]))
    others = "".join(
        f'<li><a href="/download/{html.escape(a["target"])}">{html.escape(a["short"])} <small>{mb(a["size"])}</small></a></li>'
        for a in d["assets"][1:]
    )
    sums_html = "".join(
        f"<dt>{html.escape(a['name'])}</dt><dd><code>{html.escape(a['sha256'])}</code></dd>"
        for a in d["assets"] if a["sha256"]
    )
    sums_md = "\n".join(f"- `{a['name']}` — SHA-256 `{a['sha256']}`" for a in d["assets"] if a["sha256"])
    files_md = "\n".join(
        f"- {a['short']}: https://{CONFIG['domain']}/download/{a['target']} ({a['name']}, {mb(a['size'])})"
        for a in d["assets"]
    )
    return {
        "version.txt": d["version"].lstrip("v"),
        "size.txt": mb(first["size"]),
        "date.txt": day,
        "sha.txt": first["sha256"],
        "meta.html": html.escape(meta),
        "others.html": others,
        "checksums.html": sums_html,
        "checksums.md": sums_md,
        "files.md": files_md,
    }


def download_conf(d):
    lines = [f"# Written by {CONFIG['slug']}-latest.py - do not edit."]
    for a in d["assets"]:
        lines.append(f"location = /download/{a['target']} {{\n    add_header Cache-Control \"no-store\" always;\n"
                     f"    return 302 {a['url']};\n}}")
    ids = {a["target"] for a in d["assets"]}
    var = CONFIG["nginx_var"]
    # /download follows the visitor's platform (map in the vhost); a platform without an asset in this
    # release falls back to the first target.
    lines.append(f"location = /download {{\n    add_header Cache-Control \"no-store\" always;\n"
                 f"    add_header Vary \"User-Agent\" always;\n    return 302 /download/${var}_target;\n}}")
    fallbacks = [t["id"] for t in CONFIG["targets"] if t["id"] not in ids]
    for missing in fallbacks:
        lines.append(f"location = /download/{missing} {{\n    return 302 /download/{d['assets'][0]['target']};\n}}")
    return "\n".join(lines) + "\n"


def same(path, text):
    try:
        with open(path) as f:
            return f.read() == text
    except FileNotFoundError:
        return False


def write_if_changed(path, text, mode=0o644):
    if same(path, text):
        return False
    fd, tmp = tempfile.mkstemp(dir=os.path.dirname(path), prefix=".latest-")
    with os.fdopen(fd, "w") as f:
        f.write(text)
    os.chmod(tmp, mode)
    os.replace(tmp, path)
    return True


def main():
    try:
        d = fetch_release()
    except Exception as e:  # network, rate limit, malformed release: keep the last good state
        print(f"{CONFIG['slug']}-latest: keeping previous state ({e})", file=sys.stderr)
        return 1
    changed_json = write_if_changed(os.path.join(WEBROOT, "latest.json"), json.dumps(d, indent=2) + "\n")
    ssi_dir = os.path.join(WEBROOT, "ssi")
    os.makedirs(ssi_dir, exist_ok=True)
    changed_ssi = False
    for name, text in ssi_fragments(d).items():
        changed_ssi |= write_if_changed(os.path.join(ssi_dir, name), text)
    try:
        changed_log = write_if_changed(os.path.join(WEBROOT, "changelog.md"), fetch_changelog())
    except Exception as e:  # the dialog keeps showing the last good copy
        print(f"{CONFIG['slug']}-latest: changelog not refreshed ({e})", file=sys.stderr)
        changed_log = False

    conf = download_conf(d)
    changed_conf = False
    if not same(NGINX_INC, conf) and not os.environ.get("SITE_NO_NGINX"):
        if subprocess.run(["pgrep", "-x", "certbot"], capture_output=True).returncode == 0:
            print(f"{CONFIG['slug']}-latest: certbot running, nginx update deferred", file=sys.stderr)
            return 0
        backup = open(NGINX_INC).read() if os.path.exists(NGINX_INC) else None
        write_if_changed(NGINX_INC, conf)
        test = subprocess.run(["nginx", "-t"], capture_output=True, text=True)
        if test.returncode != 0:
            print(test.stderr, file=sys.stderr)
            if backup is None:
                os.remove(NGINX_INC)
            else:
                write_if_changed(NGINX_INC, backup)
            return 1
        subprocess.run(["systemctl", "reload", "nginx"], check=True)
        changed_conf = True
    elif os.environ.get("SITE_NO_NGINX"):
        write_if_changed(NGINX_INC, conf)
    print(
        f"{CONFIG['slug']}-latest: {d['version']} targets={','.join(a['target'] for a in d['assets'])} "
        f"json={'new' if changed_json else 'same'} ssi={'new' if changed_ssi else 'same'} "
        f"changelog={'new' if changed_log else 'same'} nginx={'reloaded' if changed_conf else 'same'}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
