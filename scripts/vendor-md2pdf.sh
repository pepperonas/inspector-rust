#!/usr/bin/env bash
# Vendor the md2pdf render assets from mrxdown (the reference renderer).
# Re-run after bumping mrxdown's highlight.js / KaTeX / Mermaid.
#   bash scripts/vendor-md2pdf.sh [path-to-mrxdown]
set -euo pipefail
SRC="${1:-$HOME/claude/mrxdown}"
DST="$(cd "$(dirname "$0")/.." && pwd)/core/rust-lib/assets/md2pdf"
mkdir -p "$DST"
cp "$SRC/pdf-templates/default.css" "$DST/mrxdown-default.css"
cp "$SRC/vendor/highlight.min.js" "$DST/highlight.min.js"
cp "$SRC/vendor/katex/katex.min.js" "$DST/katex.min.js"
cp "$SRC/vendor/mermaid.min.js" "$DST/mermaid.min.js"
# KaTeX fonts inlined as woff2 data URIs: the PDF page loads from a string,
# so a relative url(fonts/…) could never resolve. Other formats are dropped
# (WebKit reads woff2).
python3 - "$SRC/vendor/katex/katex.min.css" "$SRC/vendor/katex/fonts" "$DST/katex.inline.css" <<'PY'
import base64, re, sys, pathlib
css = pathlib.Path(sys.argv[1]).read_text()
fonts = pathlib.Path(sys.argv[2])
def src(m):
    parts = []
    for u in re.findall(r'url\(([^)]+)\)\s*format\("?woff2"?\)', m.group(0)):
        f = fonts / pathlib.Path(u.strip('\'"')).name
        parts.append('url(data:font/woff2;base64,%s) format("woff2")' % base64.b64encode(f.read_bytes()).decode())
    return 'src:' + ','.join(parts)
css = re.sub(r'src:[^;}]+', src, css)
pathlib.Path(sys.argv[3]).write_text(css)
PY
{
  echo "md2pdf render assets, vendored from mrxdown ($(cd "$SRC" && git rev-parse --short HEAD 2>/dev/null || echo unknown))."
  echo "mrxdown-default.css — MIT, (c) Martin Pfeffer (mrxdown)."
  echo "highlight.min.js — highlight.js, BSD-3-Clause."
  echo "katex.min.js, katex.inline.css — KaTeX $(node -p "require('$SRC/node_modules/katex/package.json').version" 2>/dev/null), MIT; fonts SIL OFL 1.1."
  echo "mermaid.min.js — Mermaid $(node -p "require('$SRC/node_modules/mermaid/package.json').version" 2>/dev/null), MIT."
} > "$DST/THIRDPARTY.txt"
ls -la "$DST"
