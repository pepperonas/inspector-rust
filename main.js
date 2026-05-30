const { app, BrowserWindow, Menu, dialog, ipcMain, shell } = require('electron');
const path = require('path');
const fs = require('fs').promises;
const fsSync = require('fs');
const fsWatchers = new Map();
const packageJson = require('./package.json');

// highlight.js is only needed during PDF export but pulls ~9 MB of language
// definitions into memory at require() time. Lazy-load on first use.
let _hljs = null;
function getHljs() {
    if (!_hljs) _hljs = require('highlight.js');
    return _hljs;
}

// --- Settings & Recent Files Persistence ---
const userDataPath = app.getPath('userData');
const settingsPath = path.join(userDataPath, 'settings.json');
const recentFilesPath = path.join(userDataPath, 'recent-files.json');

function loadSettingsFromDisk() {
    try {
        if (fsSync.existsSync(settingsPath)) {
            const data = fsSync.readFileSync(settingsPath, 'utf-8');
            return JSON.parse(data);
        }
    } catch (error) {
        console.error('Error loading settings:', error);
    }
    return {};
}

async function saveSettingsToDisk() {
    try {
        await fs.writeFile(settingsPath, JSON.stringify(settings, null, 2), 'utf-8');
    } catch (error) {
        console.error('Error saving settings:', error);
    }
}

function loadRecentFilesFromDisk() {
    try {
        if (fsSync.existsSync(recentFilesPath)) {
            const data = fsSync.readFileSync(recentFilesPath, 'utf-8');
            return JSON.parse(data);
        }
    } catch (error) {
        console.error('Error loading recent files:', error);
    }
    return [];
}

// Async existence filter with per-file timeout. Used post-startup so that
// recent files on offline network drives don't block app launch for 20-30s.
async function pruneMissingRecentFiles() {
    const PER_FILE_TIMEOUT = 300; // ms — short enough that pruning fully completes quickly
    const checks = recentFiles.map(async (f) => {
        const check = fs.access(f).then(() => true).catch(() => false);
        const timeout = new Promise((resolve) => setTimeout(() => resolve(null), PER_FILE_TIMEOUT));
        const result = await Promise.race([check, timeout]);
        // Keep the entry on timeout (unknown) — only drop confirmed-missing files
        return result === false ? null : f;
    });
    const resolved = await Promise.all(checks);
    const filtered = resolved.filter(f => f !== null);
    if (filtered.length !== recentFiles.length) {
        recentFiles.length = 0;
        recentFiles.push(...filtered);
        saveRecentFilesToDisk();
        updateRecentFilesMenu();
    }
}

async function saveRecentFilesToDisk() {
    try {
        await fs.writeFile(recentFilesPath, JSON.stringify(recentFiles, null, 2), 'utf-8');
    } catch (error) {
        console.error('Error saving recent files:', error);
    }
}

// --- PDF Template System ---
// Templates live in pdf-templates/<name>.css alongside a templates.json manifest.
// Selection order: frontmatter `template:` > settings default > 'default'.
// Templates may opt into a title page by providing .mrx-titlepage styles.

const pdfTemplatesDir = path.join(__dirname, 'pdf-templates');
let _pdfTemplatesManifest = null;
const _pdfTemplateCssCache = new Map(); // name -> css string

function getPdfTemplatesManifest() {
    if (_pdfTemplatesManifest) return _pdfTemplatesManifest;
    try {
        const raw = fsSync.readFileSync(path.join(pdfTemplatesDir, 'templates.json'), 'utf-8');
        _pdfTemplatesManifest = JSON.parse(raw);
    } catch (err) {
        console.warn('PDF templates manifest missing or invalid:', err.message);
        _pdfTemplatesManifest = { default: { name: 'Standard', description: '', supportsTitlePage: true } };
    }
    return _pdfTemplatesManifest;
}

function loadPdfTemplateCss(name) {
    // Guard against traversal via user-supplied frontmatter values
    if (!/^[a-z0-9_-]+$/i.test(name || '')) name = 'default';
    if (_pdfTemplateCssCache.has(name)) return _pdfTemplateCssCache.get(name);
    const filePath = path.join(pdfTemplatesDir, `${name}.css`);
    try {
        const css = fsSync.readFileSync(filePath, 'utf-8');
        _pdfTemplateCssCache.set(name, css);
        return css;
    } catch (err) {
        if (name !== 'default') {
            console.warn(`PDF template '${name}' not found, falling back to 'default'`);
            return loadPdfTemplateCss('default');
        }
        console.error('Could not load any PDF template CSS:', err);
        return '';
    }
}

// Minimal YAML-lite parser for the frontmatter we care about (key: value pairs,
// block scalars via `|` or `>`, simple comments). Handles multi-line abstracts.
// NOT a full YAML implementation — just enough for frontmatter in practice.
function parseFrontmatterYaml(raw) {
    if (!raw) return {};
    const out = {};
    const lines = raw.split(/\r?\n/);
    let i = 0;
    while (i < lines.length) {
        const line = lines[i];
        if (!line.trim() || line.trim().startsWith('#')) { i++; continue; }
        const m = line.match(/^([A-Za-z0-9_-]+)\s*:\s*(.*)$/);
        if (!m) { i++; continue; }
        const key = m[1];
        let val = m[2].trim();
        if (val === '|' || val === '>') {
            // Block scalar — collect until dedent or blank line
            i++;
            const indent = (lines[i] || '').match(/^(\s*)/)[1].length || 2;
            const collected = [];
            while (i < lines.length) {
                const l = lines[i];
                if (!l.trim()) { collected.push(''); i++; continue; }
                if (l.match(/^(\s*)/)[1].length < indent) break;
                collected.push(l.slice(indent));
                i++;
            }
            out[key] = val === '>' ? collected.join(' ').trim() : collected.join('\n').trimEnd();
            continue;
        }
        // Strip surrounding quotes
        val = val.replace(/^"(.*)"$/, '$1').replace(/^'(.*)'$/, '$1');
        out[key] = val;
        i++;
    }
    return out;
}

function extractFrontmatter(markdown) {
    const m = (markdown || '').match(/^---\r?\n([\s\S]*?)\r?\n---\r?\n/);
    if (!m) return { frontmatter: {}, body: markdown || '' };
    return {
        frontmatter: parseFrontmatterYaml(m[1]),
        body: markdown.slice(m[0].length)
    };
}

// Escape HTML for title-page text fields
function escHtml(s) {
    return String(s || '')
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;')
        .replace(/"/g, '&quot;');
}

function renderTitlePage(frontmatter, templateName) {
    const manifest = getPdfTemplatesManifest();
    const tpl = manifest[templateName] || manifest.default || {};
    if (!tpl.supportsTitlePage) return '';
    if (!frontmatter || !frontmatter.title) return '';
    const parts = [];
    parts.push(`<h1>${escHtml(frontmatter.title)}</h1>`);
    if (frontmatter.subtitle) parts.push(`<div class="mrx-subtitle">${escHtml(frontmatter.subtitle)}</div>`);
    if (frontmatter.author) parts.push(`<div class="mrx-author">${escHtml(frontmatter.author)}</div>`);
    if (frontmatter.affiliation) parts.push(`<div class="mrx-affiliation">${escHtml(frontmatter.affiliation)}</div>`);
    if (frontmatter.date) parts.push(`<div class="mrx-date">${escHtml(frontmatter.date)}</div>`);
    if (frontmatter.abstract) {
        parts.push('<div class="mrx-abstract">');
        parts.push('<div class="mrx-abstract-label">Abstract</div>');
        parts.push(escHtml(frontmatter.abstract).replace(/\n/g, '<br>'));
        parts.push('</div>');
    }
    return `<div class="mrx-titlepage">${parts.join('')}</div>`;
}

function resolvePdfTemplateName(frontmatter, fallback) {
    const manifest = getPdfTemplatesManifest();
    const candidates = [
        frontmatter && frontmatter.template,
        fallback,
        'default'
    ];
    for (const c of candidates) {
        if (c && manifest[c]) return c;
    }
    return 'default';
}

// Shared PDF stylesheet — kept as a thin wrapper that loads the 'default' template
// for callers that predate the template system. New code should prefer loadPdfTemplateCss().
function getPdfStylesheet() {
    return loadPdfTemplateCss('default');
}

// Original (inlined) stylesheet replaced by pdf-templates/default.css. Kept in
// git history for reference. The stub below is only hit if the templates dir is
// missing at runtime — which shouldn't happen given electron-builder's `files`
// config, but we fall through gracefully just in case.
function _getPdfStylesheetFallback_DO_NOT_CALL() {
    return `
        @page {
            margin: 20mm 15mm;
            size: A4 portrait;
            @bottom-center {
                content: counter(page);
                font-size: 9pt;
                color: #999;
            }
        }
        * { -webkit-print-color-adjust: exact !important; print-color-adjust: exact !important; }
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial,
                         'Noto Sans', 'Apple Color Emoji', 'Segoe UI Emoji', 'Segoe UI Symbol',
                         'Noto Color Emoji', sans-serif;
            color: #1a1a1a !important;
            background: #fff !important;
            max-width: 800px;
            margin: 0 auto;
            padding: 20px;
            line-height: 1.7;
            font-size: 11pt;
            orphans: 3;
            widows: 3;
            text-rendering: optimizeLegibility;
        }
        h1, h2, h3, h4, h5, h6 {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial,
                         'Noto Sans', 'Apple Color Emoji', 'Segoe UI Emoji', 'Segoe UI Symbol',
                         'Noto Color Emoji', sans-serif;
            color: #1a1a1a !important;
            font-weight: 600;
            margin-top: 2rem;
            margin-bottom: 1rem;
            page-break-after: avoid;
            page-break-inside: avoid;
            text-rendering: optimizeLegibility;
            -webkit-font-smoothing: subpixel-antialiased;
            font-feature-settings: "kern" 1;
            white-space: pre-wrap;
        }
        h1 { font-size: 2.25rem; border-bottom: 2px solid #333; padding-bottom: 0.5rem; margin-top: 0; page-break-before: auto; }
        h2 { font-size: 1.875rem; }
        h3 { font-size: 1.5rem; }
        h4 { font-size: 1.25rem; }
        h5 { font-size: 1.125rem; }
        h6 { font-size: 1rem; color: #666 !important; }
        p { color: #1a1a1a !important; margin: 0; text-align: justify; hyphens: auto; }
        p + p { margin-top: 1rem; }
        h1 + p, h2 + p, h3 + p, h4 + p, h5 + p, h6 + p { margin-top: 0; }
        .line-break { display: block; height: 0; }
        .line-break + .line-break { height: 1em; }
        br { display: block; height: 0; }
        br + br { height: 1em; }
        p:empty { margin: 1em 0; }
        strong, b { color: #1a1a1a !important; font-weight: 700; }
        em, i { color: #1a1a1a !important; font-style: italic; }
        code {
            background: #f5f5f5 !important;
            color: #d14 !important;
            padding: 2px 6px;
            border-radius: 3px;
            font-family: 'SF Mono', Monaco, 'Cascadia Code', 'Courier New', monospace;
            font-size: 0.9em;
            border: 1px solid #e0e0e0;
        }
        pre {
            background: #f8f8f8 !important;
            color: #1a1a1a !important;
            padding: 16px;
            border-radius: 6px;
            overflow-x: auto;
            border: 1px solid #ddd;
            margin: 1em 0;
            page-break-inside: avoid;
            line-height: 1.5;
        }
        pre code { background: none !important; color: #1a1a1a !important; padding: 0; border: none; font-size: 0.85em; }
        blockquote {
            border-left: 4px solid #666;
            padding-left: 16px; padding-top: 8px; padding-bottom: 8px;
            color: #555 !important;
            margin: 1.2em 0;
            font-style: italic;
            background: #f9f9f9;
            page-break-inside: avoid;
        }
        ul, ol { color: #1a1a1a !important; margin-bottom: 1rem; padding-left: 1.5rem; }
        li { color: #1a1a1a !important; margin-bottom: 0.5rem; page-break-inside: avoid; }
        li > ul, li > ol { margin-top: 0.3em; margin-bottom: 0.3em; }
        li input[type="checkbox"] { margin-right: 0.5em; }
        a { color: #0066cc !important; text-decoration: underline; word-wrap: break-word; }
        a[href^="#"] { color: #0066cc !important; text-decoration: underline; cursor: pointer; }
        h1[id], h2[id], h3[id], h4[id], h5[id], h6[id] { scroll-margin-top: 2em; }
        table { border-collapse: collapse; width: 100%; margin: 1.2em 0; page-break-inside: auto; font-size: 0.9em; }
        tr { page-break-inside: avoid; page-break-after: auto; }
        thead { display: table-header-group; }
        th, td { border: 1px solid #ddd; padding: 10px 12px; text-align: left; color: #1a1a1a !important; }
        th { background: #f5f5f5 !important; color: #1a1a1a !important; font-weight: 600; }
        tr:nth-child(even) { background: #fafafa !important; }
        img { max-width: 100%; height: auto; margin: 1.5em 0; page-break-inside: avoid; }
        hr { border: none; border-top: 2px solid #ddd; margin: 2em 0; page-break-after: avoid; }
        @media print { body { margin: 0; padding: 0; } a { text-decoration: underline; } }
    `;
}

// D3: Apply syntax highlighting to code blocks for PDF
function highlightCodeBlocks(htmlContent) {
    return htmlContent.replace(/<pre><code class="language-(\w+)">([\s\S]*?)<\/code><\/pre>/g,
        (match, lang, code) => {
            // Mermaid blocks are rendered in the preview as SVG placeholders; the PDF
            // export uses the renderer's already-rendered HTML, so the language-mermaid
            // block only appears in CLI mode. Skip to avoid hljs warning + messy output.
            if (lang === 'mermaid') return match;
            // Unknown-language guard — highlight.js warns loudly on unregistered langs,
            // so we check first and fall back to untouched markup for those.
            if (!getHljs().getLanguage(lang)) return match;
            try {
                const decoded = code.replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&amp;/g, '&').replace(/&#39;/g, "'").replace(/&quot;/g, '"');
                const result = getHljs().highlight(decoded, { language: lang, ignoreIllegals: true });
                return '<pre><code class="hljs language-' + lang + '">' + result.value + '</code></pre>';
            } catch (e) {
                return match;
            }
        }
    );
}

function getHighlightCss() {
    return `
        .hljs { color: #1a1a1a; }
        .hljs-keyword, .hljs-selector-tag, .hljs-built_in { color: #a626a4; font-weight: 600; }
        .hljs-string, .hljs-attr { color: #50a14f; }
        .hljs-number, .hljs-literal { color: #986801; }
        .hljs-comment { color: #999; font-style: italic; }
        .hljs-function .hljs-title, .hljs-title.function_ { color: #4078f2; }
        .hljs-class .hljs-title, .hljs-title.class_ { color: #c18401; }
        .hljs-type, .hljs-params { color: #c18401; }
        .hljs-meta, .hljs-tag { color: #e45649; }
        .hljs-variable, .hljs-template-variable { color: #e45649; }
        .hljs-regexp { color: #50a14f; }
        .hljs-symbol, .hljs-bullet { color: #4078f2; }
    `;
}

// Server-side KaTeX for CLI mode. The renderer's browser-side KaTeX can't run in
// headless-CLI context, so we pre-process $...$ / $$...$$ into KaTeX HTML here.
// Matches the delimiter set used by renderMathInPreview() in renderer.js.
let _katex = null;
function renderMathForCLI(markdown) {
    if (!_katex) {
        try { _katex = require('katex'); } catch { return markdown; }
    }
    // Block math first (greedy-safe because we match $$ pairs).
    markdown = markdown.replace(/\$\$([\s\S]+?)\$\$/g, (_, expr) => {
        try { return _katex.renderToString(expr.trim(), { displayMode: true, throwOnError: false }); }
        catch { return _; }
    });
    // Inline math — require a non-dollar on both sides to avoid matching `$5.00 or $10`.
    // Single $...$ pairs on one line, no unescaped $ inside.
    markdown = markdown.replace(/(^|[^\\$])\$([^$\n]+?)\$(?!\d)/g, (m, prefix, expr) => {
        try { return prefix + _katex.renderToString(expr, { displayMode: false, throwOnError: false }); }
        catch { return m; }
    });
    return markdown;
}

// KaTeX CSS is inlined into the PDF stylesheet so rendered math is correctly laid out.
// Font URLs are rewritten to absolute file:// paths that resolve both from the on-disk
// vendor tree (dev) and from inside app.asar (packaged). Cached after first read.
let _katexCssCache = null;
function getKatexCss() {
    if (_katexCssCache !== null) return _katexCssCache;
    try {
        const katexCssPath = path.join(__dirname, 'vendor', 'katex', 'katex.min.css');
        let css = fsSync.readFileSync(katexCssPath, 'utf-8');
        const fontsDir = path.join(__dirname, 'vendor', 'katex', 'fonts');
        // Rewrite relative url(./fonts/X) and url(fonts/X) and url("fonts/X") to absolute file://
        css = css.replace(/url\((['"]?)(?:\.\/)?fonts\//g, (_, quote) => `url(${quote}file://${fontsDir}/`);
        _katexCssCache = css;
    } catch (err) {
        console.warn('Could not load KaTeX CSS for PDF:', err.message);
        _katexCssCache = '';
    }
    return _katexCssCache;
}

// buildPdfHtml accepts either a bare bodyContent string (legacy) or an options
// object with { bodyContent, template, frontmatter }. Template selection falls
// through frontmatter.template -> settings.pdfTemplate -> 'default'.
function buildPdfHtml(bodyContentOrOpts) {
    const opts = typeof bodyContentOrOpts === 'string'
        ? { bodyContent: bodyContentOrOpts }
        : (bodyContentOrOpts || {});
    const bodyContent = opts.bodyContent || '';
    const frontmatter = opts.frontmatter || {};
    const fallback = opts.template || (settings && settings.pdfTemplate) || 'default';
    const templateName = resolvePdfTemplateName(frontmatter, fallback);

    const highlightedContent = highlightCodeBlocks(bodyContent);
    const titlePage = renderTitlePage(frontmatter, templateName);
    const stylesheet = loadPdfTemplateCss(templateName);

    return '<!DOCTYPE html>\n<html lang="de">\n<head>\n    <meta charset="UTF-8">\n    <meta name="viewport" content="width=device-width, initial-scale=1.0">\n    <style>' + stylesheet + getHighlightCss() + getKatexCss() + '</style>\n</head>\n<body>' + titlePage + highlightedContent + '</body>\n</html>';
}

// Helper function to convert images to base64 for PDF export
async function convertImagesToBase64(htmlContent) {
    const imgRegex = /<img[^>]+src=["']([^"']+)["'][^>]*>/g;
    let match;
    const imagePromises = [];

    while ((match = imgRegex.exec(htmlContent)) !== null) {
        const fullImgTag = match[0];
        const imgSrc = match[1];
        
        // Skip if already base64
        if (imgSrc.startsWith('data:')) {
            continue;
        }

        imagePromises.push(
            convertImageToBase64(imgSrc).then(base64 => ({
                originalTag: fullImgTag,
                originalSrc: imgSrc,
                base64: base64
            })).catch(err => {
                console.log(`Failed to convert image ${imgSrc}:`, err);
                return null;
            })
        );
    }

    const results = await Promise.all(imagePromises);
    
    for (const result of results) {
        if (result && result.base64) {
            htmlContent = htmlContent.replace(
                result.originalTag,
                result.originalTag.replace(result.originalSrc, result.base64)
            );
        }
    }

    return htmlContent;
}

async function convertImageToBase64(imagePath) {
    try {
        // Handle different types of paths
        let fullPath;
        
        if (imagePath.startsWith('http://') || imagePath.startsWith('https://')) {
            // A4: Web URL with timeout, size limit, and redirect limit
            const https = require('https');
            const http = require('http');
            const MAX_IMAGE_SIZE = 10 * 1024 * 1024; // 10 MB
            const FETCH_TIMEOUT = 10000; // 10s
            const MAX_REDIRECTS = 5;

            return new Promise((resolve, reject) => {
                let redirectCount = 0;

                function fetchUrl(url) {
                    const client = url.startsWith('https://') ? https : http;
                    const req = client.get(url, { timeout: FETCH_TIMEOUT }, (response) => {
                        // Handle redirects
                        if ([301, 302, 303, 307, 308].includes(response.statusCode) && response.headers.location) {
                            redirectCount++;
                            if (redirectCount > MAX_REDIRECTS) {
                                reject(new Error(`Too many redirects (>${MAX_REDIRECTS})`));
                                return;
                            }
                            let redirectUrl = response.headers.location;
                            if (redirectUrl.startsWith('/')) {
                                const parsed = new URL(url);
                                redirectUrl = `${parsed.protocol}//${parsed.host}${redirectUrl}`;
                            }
                            response.resume(); // consume response
                            fetchUrl(redirectUrl);
                            return;
                        }

                        // Check content-length header
                        const contentLength = parseInt(response.headers['content-length'], 10);
                        if (contentLength && contentLength > MAX_IMAGE_SIZE) {
                            response.resume();
                            resolve(null); // Skip oversized image
                            return;
                        }

                        const chunks = [];
                        let totalSize = 0;
                        response.on('data', chunk => {
                            totalSize += chunk.length;
                            if (totalSize > MAX_IMAGE_SIZE) {
                                response.destroy();
                                resolve(null); // Skip oversized image
                                return;
                            }
                            chunks.push(chunk);
                        });
                        response.on('end', () => {
                            const buffer = Buffer.concat(chunks);
                            const mimeType = response.headers['content-type'] || 'image/png';
                            const base64 = buffer.toString('base64');
                            resolve(`data:${mimeType};base64,${base64}`);
                        });
                    });
                    req.on('error', reject);
                    req.on('timeout', () => {
                        req.destroy();
                        resolve(null); // Skip timed-out image
                    });
                }

                fetchUrl(imagePath);
            });
        } else if (path.isAbsolute(imagePath)) {
            // Absolute path
            fullPath = path.resolve(imagePath);
        } else {
            // Relative path - resolve relative to current file or working directory
            if (currentFilePath) {
                fullPath = path.resolve(path.dirname(currentFilePath), imagePath);
            } else {
                fullPath = path.resolve(imagePath);
            }
        }

        // Path traversal guard: fullPath must stay within the Markdown file's directory subtree.
        // Blocks malicious markdown like ![](../../../../etc/passwd) or ![](/Users/victim/.ssh/id_rsa)
        // from leaking arbitrary files into the exported PDF. If no currentFilePath is known
        // (CLI without source context), falls back to process.cwd() as the root.
        const baseDir = currentFilePath ? path.resolve(path.dirname(currentFilePath)) : process.cwd();
        const rel = path.relative(baseDir, fullPath);
        if (rel.startsWith('..') || path.isAbsolute(rel)) {
            console.log(`Blocked out-of-tree image path: ${imagePath} (resolved to ${fullPath}, base ${baseDir})`);
            return null;
        }

        // Read local file
        const imageBuffer = await fs.readFile(fullPath);
        const ext = path.extname(fullPath).toLowerCase();
        
        let mimeType = 'image/png';
        if (ext === '.jpg' || ext === '.jpeg') mimeType = 'image/jpeg';
        else if (ext === '.gif') mimeType = 'image/gif';
        else if (ext === '.svg') mimeType = 'image/svg+xml';
        else if (ext === '.webp') mimeType = 'image/webp';

        const base64 = imageBuffer.toString('base64');
        return `data:${mimeType};base64,${base64}`;
    } catch (error) {
        console.log(`Error converting image ${imagePath}:`, error);
        return null;
    }
}

let mainWindow;
let currentFilePath = null;
let documentEdited = false;
let lastSaveDirectory = null;
let pendingOpenFile = null; // A7: file to open when app finishes launching
let isCleanShutdown = false; // Prevents beforeunload race: blocks session save during clean close

// Store for app settings - load from disk, merge with defaults
const defaultSettings = {
    theme: 'dark',
    fontSize: 14,
    autoSave: false,
    autoSaveInterval: 5,
    showLineNumbers: true,
    wordWrap: true,
    tabSize: 4,
    pdfTemplate: 'default'
};
let settings = { ...defaultSettings, ...loadSettingsFromDisk() };
const recentFiles = loadRecentFilesFromDisk();

function buildRecentFilesSubmenu() {
    const items = recentFiles.map(file => ({
        label: path.basename(file),
        sublabel: file,
        click: () => {
            if (mainWindow) {
                mainWindow.webContents.send('menu-action', { action: 'open-recent', filePath: file });
            }
        }
    }));
    if (items.length > 0) {
        items.push({ type: 'separator' });
        items.push({
            label: 'Liste leeren',
            click: () => {
                recentFiles.length = 0;
                saveRecentFilesToDisk();
                updateRecentFilesMenu();
            }
        });
    } else {
        items.push({ label: 'Keine Dateien', enabled: false });
    }
    return items;
}

function updateRecentFilesMenu() {
    const menu = Menu.getApplicationMenu();
    if (!menu) return;
    // Rebuild the entire menu to update the recent files submenu
    // (Electron menus are immutable after creation)
    createApplicationMenu();
}

function createApplicationMenu() {
    const template = getMenuTemplate();
    const menu = Menu.buildFromTemplate(template);
    Menu.setApplicationMenu(menu);
}

function getMenuTemplate() {
    const isMac = process.platform === 'darwin';
    const template = [];

    // macOS app menu (first menu item is handled specially by macOS)
    if (isMac) {
        template.push({
            label: 'MrxDown',
            submenu: [
                {
                    label: 'Über MrxDown',
                    click: () => {
                        if (mainWindow) mainWindow.webContents.send('menu-action', { action: 'show-about', version: packageJson.version });
                    }
                },
                {
                    label: 'Nach Updates suchen…',
                    click: () => triggerManualUpdateCheck()
                },
                { type: 'separator' },
                { role: 'services' },
                { type: 'separator' },
                { role: 'hide' },
                { role: 'hideOthers' },
                { role: 'unhide' },
                { type: 'separator' },
                { role: 'quit' }
            ]
        });
    }

    // File menu — on Windows, add About + Quit here since there's no app menu
    const fileSubmenu = [
        {
            label: 'Neue Datei',
            accelerator: 'CmdOrCtrl+N',
            click: () => { if (mainWindow) mainWindow.webContents.send('menu-action', { action: 'new-file' }); }
        },
        {
            label: 'Neues Fenster',
            accelerator: 'CmdOrCtrl+Shift+N',
            click: () => { createWindow(); }
        },
        { type: 'separator' },
        {
            label: 'Öffnen...',
            accelerator: 'CmdOrCtrl+O',
            click: () => { if (mainWindow) mainWindow.webContents.send('menu-action', { action: 'open-file' }); }
        },
        {
            label: 'Zuletzt verwendet',
            id: 'recent-files',
            submenu: buildRecentFilesSubmenu()
        },
        { type: 'separator' },
        {
            label: 'Speichern',
            accelerator: 'CmdOrCtrl+S',
            click: () => { if (mainWindow) mainWindow.webContents.send('menu-action', { action: 'save-file' }); }
        },
        {
            label: 'Speichern unter...',
            accelerator: 'CmdOrCtrl+Shift+S',
            click: () => { if (mainWindow) mainWindow.webContents.send('menu-action', { action: 'save-file-as' }); }
        },
        { type: 'separator' },
        {
            label: 'Als HTML exportieren',
            accelerator: 'CmdOrCtrl+E',
            click: () => { if (mainWindow) mainWindow.webContents.send('menu-action', { action: 'export-html' }); }
        },
        {
            label: 'Als PDF exportieren',
            accelerator: 'CmdOrCtrl+P',
            click: () => { if (mainWindow) mainWindow.webContents.send('menu-action', { action: 'export-pdf' }); }
        }
    ];

    if (!isMac) {
        fileSubmenu.push(
            { type: 'separator' },
            {
                label: 'Über MrxDown',
                click: () => {
                    if (mainWindow) mainWindow.webContents.send('menu-action', { action: 'show-about', version: packageJson.version });
                }
            },
            {
                label: 'Nach Updates suchen…',
                click: () => triggerManualUpdateCheck()
            },
            { type: 'separator' },
            { role: 'quit', label: 'Beenden' }
        );
    }

    template.push({ label: 'Datei', submenu: fileSubmenu });

    template.push({
        label: 'Bearbeiten',
        submenu: [
            { role: 'undo', label: 'Rückgängig' },
            { role: 'redo', label: 'Wiederholen' },
            { type: 'separator' },
            { role: 'cut', label: 'Ausschneiden' },
            { role: 'copy', label: 'Kopieren' },
            { role: 'paste', label: 'Einfügen' },
            { role: 'selectAll', label: 'Alles auswählen' },
            { type: 'separator' },
            {
                label: 'Suchen',
                accelerator: 'CmdOrCtrl+F',
                click: () => { if (mainWindow) mainWindow.webContents.send('menu-action', { action: 'find' }); }
            },
            {
                label: 'Ersetzen',
                accelerator: 'CmdOrCtrl+R',
                click: () => { if (mainWindow) mainWindow.webContents.send('menu-action', { action: 'replace' }); }
            },
            { type: 'separator' },
            {
                label: 'Zeile löschen',
                accelerator: 'CmdOrCtrl+Shift+K',
                click: () => { if (mainWindow) mainWindow.webContents.send('menu-action', { action: 'delete-line' }); }
            },
            {
                label: 'Zeile duplizieren',
                accelerator: 'CmdOrCtrl+D',
                click: () => { if (mainWindow) mainWindow.webContents.send('menu-action', { action: 'duplicate-line' }); }
            },
            {
                label: 'Zeile nach oben',
                accelerator: 'Alt+Up',
                click: () => { if (mainWindow) mainWindow.webContents.send('menu-action', { action: 'move-line-up' }); }
            },
            {
                label: 'Zeile nach unten',
                accelerator: 'Alt+Down',
                click: () => { if (mainWindow) mainWindow.webContents.send('menu-action', { action: 'move-line-down' }); }
            },
            {
                label: 'Kommentar umschalten',
                accelerator: 'CmdOrCtrl+/',
                click: () => { if (mainWindow) mainWindow.webContents.send('menu-action', { action: 'toggle-comment' }); }
            },
            {
                label: 'Zeile markieren',
                accelerator: 'CmdOrCtrl+L',
                click: () => { if (mainWindow) mainWindow.webContents.send('menu-action', { action: 'select-line' }); }
            }
        ]
    });

    template.push({
        label: 'Format',
        submenu: [
            {
                label: 'Fett',
                accelerator: 'CmdOrCtrl+B',
                click: () => { if (mainWindow) mainWindow.webContents.send('menu-action', { action: 'format-bold' }); }
            },
            {
                label: 'Kursiv',
                accelerator: 'CmdOrCtrl+I',
                click: () => { if (mainWindow) mainWindow.webContents.send('menu-action', { action: 'format-italic' }); }
            },
            {
                label: 'Code',
                accelerator: 'CmdOrCtrl+`',
                click: () => { if (mainWindow) mainWindow.webContents.send('menu-action', { action: 'format-code' }); }
            },
            { type: 'separator' },
            {
                label: 'Link einfügen',
                accelerator: 'CmdOrCtrl+K',
                click: () => { if (mainWindow) mainWindow.webContents.send('menu-action', { action: 'insert-link' }); }
            },
            {
                label: 'Bild einfügen',
                accelerator: 'CmdOrCtrl+Shift+I',
                click: () => { if (mainWindow) mainWindow.webContents.send('menu-action', { action: 'insert-image' }); }
            },
            {
                label: 'Tabelle einfügen',
                accelerator: 'CmdOrCtrl+T',
                click: () => { if (mainWindow) mainWindow.webContents.send('menu-action', { action: 'insert-table' }); }
            },
            { type: 'separator' },
            {
                label: 'Überschrift 1',
                accelerator: 'CmdOrCtrl+1',
                click: () => { if (mainWindow) mainWindow.webContents.send('menu-action', { action: 'heading-1' }); }
            },
            {
                label: 'Überschrift 2',
                accelerator: 'CmdOrCtrl+2',
                click: () => { if (mainWindow) mainWindow.webContents.send('menu-action', { action: 'heading-2' }); }
            },
            {
                label: 'Überschrift 3',
                accelerator: 'CmdOrCtrl+3',
                click: () => { if (mainWindow) mainWindow.webContents.send('menu-action', { action: 'heading-3' }); }
            }
        ]
    });

    template.push({
        label: 'Ansicht',
        submenu: [
            {
                label: 'Sidebar umschalten',
                accelerator: 'CmdOrCtrl+\\',
                click: () => { if (mainWindow) mainWindow.webContents.send('menu-action', { action: 'toggle-sidebar' }); }
            },
            { type: 'separator' },
            { role: 'reload', label: 'Neu laden' },
            { role: 'forceReload', label: 'Erzwungen neu laden' },
            { role: 'toggleDevTools', label: 'Entwicklertools' },
            { type: 'separator' },
            { role: 'resetZoom', label: 'Tatsächliche Größe' },
            { role: 'zoomIn', label: 'Vergrößern' },
            { role: 'zoomOut', label: 'Verkleinern' },
            { type: 'separator' },
            { role: 'togglefullscreen', label: 'Vollbild' }
        ]
    });

    // Window menu — macOS uses 'front', Windows/Linux use 'zoom'
    const windowSubmenu = [
        { role: 'minimize', label: 'Minimieren' },
        { role: 'close', label: 'Schließen' }
    ];
    if (isMac) {
        windowSubmenu.push(
            { type: 'separator' },
            { role: 'front', label: 'Alle nach vorne' }
        );
    }
    template.push({ label: 'Fenster', submenu: windowSubmenu });

    return template;
}

// E1: Window state persistence
const windowStatePath = path.join(userDataPath, 'window-state.json');

function loadWindowState() {
    try {
        if (fsSync.existsSync(windowStatePath)) {
            return JSON.parse(fsSync.readFileSync(windowStatePath, 'utf-8'));
        }
    } catch (e) { /* ignore */ }
    return null;
}

function saveWindowState() {
    if (!mainWindow) return;
    try {
        const bounds = mainWindow.getBounds();
        const state = {
            x: bounds.x, y: bounds.y,
            width: bounds.width, height: bounds.height,
            isMaximized: mainWindow.isMaximized()
        };
        fsSync.writeFileSync(windowStatePath, JSON.stringify(state, null, 2), 'utf-8');
    } catch (e) { /* ignore */ }
}

function createWindow() {
    const isMac = process.platform === 'darwin';
    const savedState = loadWindowState();
    const windowOptions = {
        width: savedState ? savedState.width : 1400,
        height: savedState ? savedState.height : 900,
        minWidth: 900,
        minHeight: 600,
        webPreferences: {
            nodeIntegration: false,
            contextIsolation: true,
            spellcheck: true,
            preload: path.join(__dirname, 'preload.js')
        },
        backgroundColor: '#2B2E3B',
        icon: path.join(__dirname, 'assets/icon.png')
    };

    // E1: Restore window position
    if (savedState && savedState.x !== undefined) {
        windowOptions.x = savedState.x;
        windowOptions.y = savedState.y;
    }

    if (isMac) {
        windowOptions.titleBarStyle = 'hiddenInset';
        windowOptions.trafficLightPosition = { x: 20, y: 20 };
    }

    mainWindow = new BrowserWindow(windowOptions);

    // E1: Restore maximized state
    if (savedState && savedState.isMaximized) {
        mainWindow.maximize();
    }

    // E1: Save window state on resize/move
    mainWindow.on('resize', saveWindowState);
    mainWindow.on('move', saveWindowState);
    mainWindow.on('maximize', saveWindowState);
    mainWindow.on('unmaximize', saveWindowState);

    mainWindow.loadFile('index.html');

    // Add platform class to body for platform-specific CSS + A7: open pending file
    mainWindow.webContents.on('did-finish-load', () => {
        mainWindow.webContents.executeJavaScript(
            `document.body.classList.add('platform-${process.platform}')`
        );
        // A7: Open file that was requested before app was ready
        if (pendingOpenFile) {
            const filePath = pendingOpenFile;
            pendingOpenFile = null;
            fs.readFile(filePath, 'utf-8').then(content => {
                mainWindow.webContents.send('file-opened', { filePath, content });
                addToRecentFiles(filePath);
            }).catch(err => console.error('Error opening pending file:', err));
        }
    });

    // Enable drag and drop
    mainWindow.webContents.on('will-navigate', (event, url) => {
        event.preventDefault();
    });

    createApplicationMenu();

    // C3/E5: Spellcheck + native editor context menu
    mainWindow.webContents.session.setSpellCheckerLanguages(['de', 'en-US']);
    mainWindow.webContents.on('context-menu', (event, params) => {
        const menuItems = [];

        // Spellcheck suggestions
        if (params.misspelledWord) {
            for (const suggestion of params.dictionarySuggestions.slice(0, 5)) {
                menuItems.push({
                    label: suggestion,
                    click: () => mainWindow.webContents.replaceMisspelling(suggestion)
                });
            }
            if (menuItems.length > 0) menuItems.push({ type: 'separator' });
            menuItems.push({
                label: 'Zum Wörterbuch hinzufügen',
                click: () => mainWindow.webContents.session.addWordToSpellCheckerDictionary(params.misspelledWord)
            });
            menuItems.push({ type: 'separator' });
        }

        // E5: Standard edit actions
        menuItems.push(
            { label: 'Ausschneiden', accelerator: 'CmdOrCtrl+X', role: 'cut' },
            { label: 'Kopieren', accelerator: 'CmdOrCtrl+C', role: 'copy' },
            { label: 'Einfügen', accelerator: 'CmdOrCtrl+V', role: 'paste' },
            { type: 'separator' },
            { label: 'Fett', accelerator: 'CmdOrCtrl+B', click: () => mainWindow.webContents.send('menu-action', { action: 'format-bold' }) },
            { label: 'Kursiv', accelerator: 'CmdOrCtrl+I', click: () => mainWindow.webContents.send('menu-action', { action: 'format-italic' }) },
            { label: 'Code', accelerator: 'CmdOrCtrl+`', click: () => mainWindow.webContents.send('menu-action', { action: 'format-code' }) },
            { label: 'Link einfügen', accelerator: 'CmdOrCtrl+K', click: () => mainWindow.webContents.send('menu-action', { action: 'insert-link' }) },
            { type: 'separator' },
            { label: 'Alles auswählen', accelerator: 'CmdOrCtrl+A', role: 'selectAll' }
        );

        const menu = Menu.buildFromTemplate(menuItems);
        menu.popup();
    });

    // A1: Race-condition-free close handler — saves synchronously before closing
    mainWindow.on('close', async (event) => {
        if (documentEdited) {
            event.preventDefault();
            const response = await dialog.showMessageBox(mainWindow, {
                type: 'question',
                buttons: ['Speichern & Beenden', 'Nicht speichern', 'Abbrechen'],
                defaultId: 0,
                cancelId: 2,
                message: 'Es gibt ungespeicherte Änderungen.',
                detail: 'Möchten Sie vor dem Beenden speichern?'
            });
            if (response.response === 0) {
                // Save then close — get content directly from renderer, no race condition
                try {
                    const tabData = await mainWindow.webContents.executeJavaScript(`
                        (function() {
                            const activeTab = tabs.find(tab => tab.id === activeTabId);
                            return {
                                content: editor.value,
                                filePath: activeTab ? activeTab.filePath : null
                            };
                        })()
                    `);
                    if (tabData.filePath) {
                        await fs.writeFile(tabData.filePath, tabData.content, 'utf-8');
                    } else {
                        // Unsaved file — show Save As dialog
                        const result = await dialog.showSaveDialog(mainWindow, {
                            filters: [
                                { name: 'Markdown', extensions: ['md'] },
                                { name: 'Text', extensions: ['txt'] }
                            ]
                        });
                        if (!result.canceled) {
                            await fs.writeFile(result.filePath, tabData.content, 'utf-8');
                        } else {
                            return; // User cancelled Save As, don't close
                        }
                    }
                } catch (err) {
                    console.error('Error saving on close:', err);
                }
                isCleanShutdown = true;
                clearSessionFile();
                documentEdited = false;
                mainWindow.close();
            } else if (response.response === 1) {
                // Don't save, just close — clear session on clean exit
                isCleanShutdown = true;
                clearSessionFile();
                documentEdited = false;
                mainWindow.close();
            }
            // response === 2: Cancel, do nothing
        } else {
            // No unsaved changes — clear session on clean exit
            isCleanShutdown = true;
            clearSessionFile();
        }
    });

    mainWindow.on('closed', () => {
        // F2: Clean up all file watchers
        for (const [filePath, watcher] of fsWatchers.entries()) {
            if (watcher && watcher.close) {
                watcher.close();
            }
        }
        fsWatchers.clear();

        mainWindow = null;
    });
}

// IPC Handlers
ipcMain.on('new-file', async (event) => {
    if (documentEdited) {
        const response = await dialog.showMessageBox(mainWindow, {
            type: 'question',
            buttons: ['Speichern', 'Nicht speichern', 'Abbrechen'],
            defaultId: 0,
            message: 'Möchten Sie die Änderungen speichern?'
        });

        if (response.response === 0) {
            // Save then create new
            event.reply('menu-action', { action: 'save-file' });
        } else if (response.response === 2) {
            return; // Cancel
        }
    }
    
    currentFilePath = null;
    documentEdited = false;
    event.reply('new-file-created', { filePath: null });
});

ipcMain.on('open-file', async (event) => {
    const result = await dialog.showOpenDialog(mainWindow, {
        properties: ['openFile', 'multiSelections'],
        defaultPath: lastSaveDirectory || undefined,
        filters: [
            { name: 'Markdown', extensions: ['md', 'markdown'] },
            { name: 'Text', extensions: ['txt'] },
            { name: 'Alle Dateien', extensions: ['*'] }
        ]
    });

    if (!result.canceled && result.filePaths.length > 0) {
        for (const filePath of result.filePaths) {
            try {
                const content = await fs.readFile(filePath, 'utf-8');
                currentFilePath = filePath;
                lastSaveDirectory = path.dirname(filePath);
                documentEdited = false;
                event.reply('file-opened', { filePath, content });
                addToRecentFiles(filePath);
            } catch (error) {
                dialog.showErrorBox('Fehler', `Datei konnte nicht geöffnet werden: ${error.message}`);
            }
        }
    }
});

// Shared save-file implementation — returns { success, filePath?, error?, cancelled? } and
// optionally emits the 'file-saved' event for callers that rely on the event bus.
// Pragmatic: attempt writeFile directly and catch EACCES, rather than access() + write (TOCTOU race).
async function doSaveFile({ content, filePath, event }) {
    const targetPath = filePath || currentFilePath;
    if (!targetPath) {
        return doSaveFileAs({ content, filePath: null, tabTitle: null, event });
    }
    try {
        await fs.writeFile(targetPath, content, 'utf-8');
        currentFilePath = targetPath;
        documentEdited = false;
        if (mainWindow) mainWindow.webContents.send('file-saved', { filePath: targetPath });
        addToRecentFiles(targetPath);
        return { success: true, filePath: targetPath };
    } catch (error) {
        if (error.code === 'EACCES' || error.code === 'EPERM' || error.code === 'EROFS') {
            const response = await dialog.showMessageBox(mainWindow, {
                type: 'warning',
                buttons: ['Speichern unter...', 'Abbrechen'],
                defaultId: 0,
                message: 'Diese Datei ist schreibgeschützt.',
                detail: 'Möchten Sie die Datei unter einem anderen Namen speichern?'
            });
            if (response.response === 0) {
                return doSaveFileAs({ content, filePath: null, tabTitle: null, event });
            }
            return { success: false, cancelled: true };
        }
        dialog.showErrorBox('Fehler', `Datei konnte nicht gespeichert werden: ${error.message}`);
        return { success: false, error: error.message };
    }
}

async function doSaveFileAs({ content, filePath, tabTitle, event }) {
    const baseFilePath = filePath || currentFilePath;
    let defaultFileName;

    if (tabTitle && tabTitle !== 'Unbenannt') {
        defaultFileName = tabTitle.endsWith('.md') || tabTitle.endsWith('.txt') ? tabTitle : `${tabTitle}.md`;
    } else if (baseFilePath) {
        defaultFileName = path.basename(baseFilePath);
    } else {
        defaultFileName = 'untitled.md';
    }

    let defaultSavePath = defaultFileName;
    if (baseFilePath) {
        defaultSavePath = path.join(path.dirname(baseFilePath), defaultFileName);
    } else if (lastSaveDirectory) {
        defaultSavePath = path.join(lastSaveDirectory, defaultFileName);
    }

    const result = await dialog.showSaveDialog(mainWindow, {
        filters: [
            { name: 'Markdown', extensions: ['md'] },
            { name: 'Text', extensions: ['txt'] }
        ],
        defaultPath: defaultSavePath
    });

    if (result.canceled) {
        return { success: false, cancelled: true };
    }
    try {
        await fs.writeFile(result.filePath, content, 'utf-8');
        currentFilePath = result.filePath;
        lastSaveDirectory = path.dirname(result.filePath);
        documentEdited = false;
        if (mainWindow) mainWindow.webContents.send('file-saved', { filePath: result.filePath });
        addToRecentFiles(result.filePath);
        return { success: true, filePath: result.filePath };
    } catch (error) {
        dialog.showErrorBox('Fehler', `Datei konnte nicht gespeichert werden: ${error.message}`);
        return { success: false, error: error.message };
    }
}

ipcMain.on('save-file', (event, { content, filePath }) => {
    doSaveFile({ content, filePath, event });
});

ipcMain.handle('save-file-sync', (event, { content, filePath }) => {
    return doSaveFile({ content, filePath, event });
});

ipcMain.handle('save-file-as-sync', (event, { content, filePath, tabTitle }) => {
    return doSaveFileAs({ content, filePath, tabTitle, event });
});

ipcMain.on('save-file-as', (event, { content, filePath, tabTitle }) => {
    doSaveFileAs({ content, filePath, tabTitle, event });
});

ipcMain.on('export-html', async (event, { content, filePath }) => {
    // Use the provided filePath from the active tab or fall back to currentFilePath
    const baseFilePath = filePath || currentFilePath;
    const defaultFileName = baseFilePath ? 
        path.basename(baseFilePath, path.extname(baseFilePath)) + '.html' : 
        'Untitled.html';
    
    const result = await dialog.showSaveDialog(mainWindow, {
        filters: [
            { name: 'HTML', extensions: ['html'] }
        ],
        defaultPath: defaultFileName
    });

    if (!result.canceled) {
        try {
            // Convert file:// image URLs to base64 data URLs
            let processedContent = content;
            const imgRegex = /<img[^>]+src="file:\/\/([^"]+)"[^>]*>/g;
            let match;
            
            while ((match = imgRegex.exec(content)) !== null) {
                const imagePath = decodeURIComponent(match[1]);
                try {
                    const imageData = await fs.readFile(imagePath);
                    const extension = path.extname(imagePath).toLowerCase().slice(1);
                    const mimeType = extension === 'jpg' ? 'jpeg' : extension;
                    const base64 = imageData.toString('base64');
                    const dataUrl = `data:image/${mimeType};base64,${base64}`;
                    processedContent = processedContent.replace(match[0], match[0].replace(`file://${match[1]}`, dataUrl));
                } catch (err) {
                    console.error(`Failed to convert image ${imagePath}:`, err);
                }
            }
            
            await fs.writeFile(result.filePath, processedContent, 'utf-8');
        } catch (error) {
            dialog.showErrorBox('Fehler', `Export fehlgeschlagen: ${error.message}`);
        }
    }
});

// D4: PDF export with user-specified options (page size, margins, orientation, TOC)
ipcMain.on('print-to-pdf-options', async (event, { filePath, pdfOptions } = {}) => {
    try {
        const opts = pdfOptions || {};
        const pageSize = opts.pageSize || 'A4';
        const landscape = opts.orientation === 'landscape';
        const margin = opts.margin !== undefined ? opts.margin : 20;
        const fontSize = opts.fontSize || 11;
        const showToc = opts.toc || false;
        const showPageNumbers = opts.pageNumbers !== false;

        const pdfWindow = new BrowserWindow({
            width: 800, height: 1000, show: false,
            webPreferences: { nodeIntegration: false, contextIsolation: true, preload: path.join(__dirname, 'preload.js') }
        });

        const pageData = await mainWindow.webContents.executeJavaScript(
            '(function() { var p = document.querySelector("#preview"); var html = p ? p.innerHTML : ""; var raw = (typeof editor !== "undefined" && editor && editor.value) ? editor.value : ""; return { previewHtml: html, rawMarkdown: raw }; })()'
        );
        let content = pageData && pageData.previewHtml ? pageData.previewHtml : '';
        const rawMarkdown = pageData && pageData.rawMarkdown ? pageData.rawMarkdown : '';
        const { frontmatter } = extractFrontmatter(rawMarkdown);

        if (!content || content.trim().length === 0) {
            dialog.showErrorBox('PDF-Export Fehler', 'Der Dokumentinhalt ist leer.');
            pdfWindow.close();
            return;
        }

        // Strip the preview's frontmatter info-box from the exported body
        content = content.replace(/<div class="frontmatter-box">[\s\S]*?<\/div>\s*(?=<)/, '');

        content = await convertImagesToBase64(content);

        // Template selection: dialog > frontmatter > settings > 'default'
        const templateName = resolvePdfTemplateName(frontmatter, opts.template || (settings && settings.pdfTemplate));

        // Build custom stylesheet on top of the selected template
        let customStyle = loadPdfTemplateCss(templateName);
        customStyle = customStyle.replace(/margin:\s*20mm\s*15mm/, 'margin: ' + margin + 'mm');
        customStyle = customStyle.replace(/size:\s*A4\s*portrait/, 'size: ' + pageSize + ' ' + (landscape ? 'landscape' : 'portrait'));
        customStyle = customStyle.replace(/font-size:\s*11pt/, 'font-size: ' + fontSize + 'pt');

        if (!showPageNumbers) {
            customStyle = customStyle.replace(/@bottom-center\s*\{[^}]+\}/, '');
        }

        // Title page from frontmatter (only if template + frontmatter support it)
        const titlePage = renderTitlePage(frontmatter, templateName);

        // D5: TOC generation
        let tocHtml = '';
        if (showToc) {
            const headingRegex = /<h([1-3])[^>]*id="([^"]*)"[^>]*>([\s\S]*?)<\/h\1>/gi;
            let match;
            const tocItems = [];
            while ((match = headingRegex.exec(content)) !== null) {
                const level = parseInt(match[1]);
                const text = match[3].replace(/<[^>]+>/g, '');
                tocItems.push({ level, text });
            }
            if (tocItems.length > 0) {
                const tocLines = ['<div class="pdf-toc"><h2>Inhaltsverzeichnis</h2><ul>'];
                for (const item of tocItems) {
                    const indent = (item.level - 1) * 20;
                    tocLines.push('<li style="margin-left:' + indent + 'px">' + item.text + '</li>');
                }
                tocLines.push('</ul></div><div style="page-break-after:always"></div>');
                tocHtml = tocLines.join('');
            }
        }

        const tocStyle = '.pdf-toc { margin-bottom: 2em; } .pdf-toc h2 { border-bottom: 2px solid #333; padding-bottom: 0.5rem; } .pdf-toc ul { list-style: none; padding: 0; } .pdf-toc li { padding: 4px 0; color: #1a1a1a; }';
        const fullHtml = '<!DOCTYPE html><html><head><meta charset="utf-8"><style>' + customStyle + tocStyle + getHighlightCss() + getKatexCss() + '</style></head><body>' + titlePage + tocHtml + content + '</body></html>';

        await pdfWindow.loadURL('data:text/html;charset=utf-8,' + encodeURIComponent(fullHtml));

        // Smart wait for images
        await pdfWindow.webContents.executeJavaScript(
            'new Promise(function(resolve) { var imgs = document.querySelectorAll("img"); var ps = Array.from(imgs).map(function(img) { if (img.complete) return Promise.resolve(); return new Promise(function(r) { img.onload = r; img.onerror = r; }); }); Promise.all(ps).then(function() { if (typeof requestIdleCallback !== "undefined") { requestIdleCallback(function() { resolve(); }, { timeout: 2000 }); } else { setTimeout(resolve, 500); } }); })'
        );

        const pdfData = await pdfWindow.webContents.printToPDF({
            marginsType: 0,
            pageSize: pageSize,
            printBackground: true,
            landscape: landscape,
            preferCSSPageSize: true,
            generateTaggedPDF: true,
            generateDocumentOutline: true
        });

        pdfWindow.close();

        const baseFilePath = filePath || currentFilePath;
        const defaultFileName = baseFilePath ?
            path.basename(baseFilePath, path.extname(baseFilePath)) + '.pdf' :
            'Untitled.pdf';

        const result = await dialog.showSaveDialog(mainWindow, {
            filters: [{ name: 'PDF', extensions: ['pdf'] }],
            defaultPath: defaultFileName
        });

        if (!result.canceled) {
            await fs.writeFile(result.filePath, pdfData);
        }
    } catch (error) {
        dialog.showErrorBox('Fehler', 'PDF-Export fehlgeschlagen: ' + error.message);
    }
});

ipcMain.on('print-to-pdf', async (event, { filePath } = {}) => {
    try {
        // Create a new window for PDF generation with proper styling
        const pdfWindow = new BrowserWindow({
            width: 800,
            height: 1000,
            show: false,
            webPreferences: {
                nodeIntegration: false,
                contextIsolation: true,
                preload: path.join(__dirname, 'preload.js')
            }
        });

        // Pull preview HTML and raw editor markdown so we can extract frontmatter
        // for template selection + title-page rendering.
        const pageData = await mainWindow.webContents.executeJavaScript(`
            (function() {
                const previewElement = document.querySelector('#preview');
                const previewHtml = previewElement ? previewElement.innerHTML : '';
                const rawMarkdown = (typeof editor !== 'undefined' && editor && editor.value) ? editor.value : '';
                return { previewHtml, rawMarkdown };
            })()
        `);
        let content = pageData && pageData.previewHtml ? pageData.previewHtml : '';
        const rawMarkdown = pageData && pageData.rawMarkdown ? pageData.rawMarkdown : '';
        const { frontmatter } = extractFrontmatter(rawMarkdown);

        // Check if content is empty
        if (!content || content.trim().length === 0) {
            console.error('PDF Export: Preview content is empty');
            dialog.showErrorBox('PDF-Export Fehler', 'Der Dokumentinhalt ist leer. Bitte schreiben Sie etwas in den Editor, bevor Sie ein PDF exportieren.');
            pdfWindow.close();
            return;
        }

        // Strip the frontmatter info-box the preview adds (not part of the exported doc).
        content = content.replace(/<div class="frontmatter-box">[\s\S]*?<\/div>\s*(?=<)/, '');

        // Convert images to base64 for embedding
        content = await convertImagesToBase64(content);

        // Keep <br> tags - styled via CSS

        // Load HTML with optimized print styles — template picked from frontmatter/settings
        await pdfWindow.loadURL(`data:text/html;charset=utf-8,${encodeURIComponent(buildPdfHtml({ bodyContent: content, frontmatter }))}`);

        // A5: Smart wait — wait for images to load + DOM idle instead of fixed 2s
        await pdfWindow.webContents.executeJavaScript(`
            new Promise(resolve => {
                const images = document.querySelectorAll('img');
                const imagePromises = Array.from(images).map(img => {
                    if (img.complete) return Promise.resolve();
                    return new Promise(r => { img.onload = r; img.onerror = r; });
                });
                Promise.all(imagePromises).then(() => {
                    if (typeof requestIdleCallback !== 'undefined') {
                        requestIdleCallback(() => resolve(), { timeout: 2000 });
                    } else {
                        setTimeout(resolve, 500);
                    }
                });
            })
        `);

        const pdfData = await pdfWindow.webContents.printToPDF({
            marginsType: 0, // Use custom margins from @page
            pageSize: 'A4',
            printBackground: true,
            landscape: false,
            preferCSSPageSize: true,
            printSelectionOnly: false,
            generateTaggedPDF: true, // Enable tagged PDF for better accessibility and structure
            generateDocumentOutline: true // Try to generate document outline from headings
        });

        pdfWindow.close();

        // Use the provided filePath from the active tab or fall back to currentFilePath
        const baseFilePath = filePath || currentFilePath;
        const defaultFileName = baseFilePath ? 
            path.basename(baseFilePath, path.extname(baseFilePath)) + '.pdf' : 
            'Untitled.pdf';
        
        const result = await dialog.showSaveDialog(mainWindow, {
            filters: [
                { name: 'PDF', extensions: ['pdf'] }
            ],
            defaultPath: defaultFileName
        });
        
        if (!result.canceled) {
            await fs.writeFile(result.filePath, pdfData);
        }
    } catch (error) {
        dialog.showErrorBox('Fehler', `PDF-Export fehlgeschlagen: ${error.message}`);
    }
});

ipcMain.on('batch-print-to-pdf', async (event, { tabData } = {}) => {
    try {
        if (!tabData || tabData.length === 0) {
            dialog.showErrorBox('Fehler', 'Keine Tabs zum Exportieren gefunden.');
            return;
        }
        
        console.log(`Starting batch PDF export for ${tabData.length} files`);
        let successCount = 0;
        let errorCount = 0;
        const errors = [];
        
        for (const tab of tabData) {
            try {
                console.log(`Exporting PDF for: ${tab.filePath}`);
                
                // Send IPC message to renderer to switch tab and render content
                mainWindow.webContents.send('batch-export-prepare-tab', {
                    filePath: tab.filePath,
                    content: tab.content
                });
                
                // Wait for the tab to be prepared and rendered.
                // Named listener so we can remove it on timeout — without this, a dangling
                // `ipcMain.once` would fire on the NEXT tab's ready event and corrupt its Promise.
                let htmlContent = await new Promise((resolve, reject) => {
                    const onReady = (event, data) => {
                        clearTimeout(timeout);
                        if (data.error) reject(new Error(data.error));
                        else resolve(data.htmlContent);
                    };
                    const timeout = setTimeout(() => {
                        ipcMain.removeListener('batch-export-tab-ready', onReady);
                        reject(new Error('Timeout waiting for tab preparation'));
                    }, 10000);
                    ipcMain.once('batch-export-tab-ready', onReady);
                });
                
                // Convert images to base64 for embedding
                htmlContent = await convertImagesToBase64(htmlContent);

                // Extract frontmatter from tab's source markdown for template selection
                const { frontmatter: tabFrontmatter } = extractFrontmatter(tab.content || '');

                // Keep <br> tags - styled via CSS

                // Create a new window for PDF generation
                const pdfWindow = new BrowserWindow({
                    width: 800,
                    height: 1000,
                    show: false,
                    webPreferences: {
                        nodeIntegration: false,
                        contextIsolation: true,
                        preload: path.join(__dirname, 'preload.js')
                    }
                });
                
                // Load HTML with shared print styles — template per tab from frontmatter
                await pdfWindow.loadURL(`data:text/html;charset=utf-8,${encodeURIComponent(buildPdfHtml({ bodyContent: htmlContent, frontmatter: tabFrontmatter }))}`);

                // A5: Smart wait — wait for images + DOM idle
                await pdfWindow.webContents.executeJavaScript(`
                    new Promise(resolve => {
                        const images = document.querySelectorAll('img');
                        const imagePromises = Array.from(images).map(img => {
                            if (img.complete) return Promise.resolve();
                            return new Promise(r => { img.onload = r; img.onerror = r; });
                        });
                        Promise.all(imagePromises).then(() => {
                            if (typeof requestIdleCallback !== 'undefined') {
                                requestIdleCallback(() => resolve(), { timeout: 2000 });
                            } else {
                                setTimeout(resolve, 500);
                            }
                        });
                    })
                `);

                const pdfData = await pdfWindow.webContents.printToPDF({
                    marginsType: 0, // Use custom margins from @page
                    pageSize: 'A4',
                    printBackground: true,
                    landscape: false,
                    preferCSSPageSize: true,
                    printSelectionOnly: false,
                    generateTaggedPDF: true, // Enable tagged PDF for better accessibility and structure
                    generateDocumentOutline: true // Try to generate document outline from headings
                });

                pdfWindow.close();
                
                // Generate PDF path in same directory as MD file
                const pdfPath = tab.filePath.replace(/\.(md|markdown)$/i, '.pdf');
                console.log(`Saving PDF to: ${pdfPath}`);
                
                await fs.writeFile(pdfPath, pdfData);
                successCount++;
                console.log(`Successfully exported: ${pdfPath}`);
                
            } catch (fileError) {
                console.error(`Error exporting ${tab.filePath}:`, fileError);
                errorCount++;
                errors.push(`${tab.title}: ${fileError.message}`);
            }
        }
        
        // Show summary
        let message = `PDF-Batch-Export abgeschlossen!\n\n`;
        message += `✅ Erfolgreich exportiert: ${successCount}\n`;
        if (errorCount > 0) {
            message += `❌ Fehler: ${errorCount}\n\n`;
            message += errors.join('\n');
        }
        
        dialog.showMessageBox(mainWindow, {
            type: errorCount > 0 ? 'warning' : 'info',
            title: 'PDF-Batch-Export',
            message: message,
            buttons: ['OK']
        });
        
    } catch (error) {
        console.error('Batch PDF export error:', error);
        dialog.showErrorBox('Fehler', `Batch-PDF-Export fehlgeschlagen: ${error.message}`);
    }
});

// Settings handlers
ipcMain.handle('get-settings', () => {
    return settings;
});

ipcMain.on('save-settings', (event, newSettings) => {
    settings = { ...settings, ...newSettings };
    saveSettingsToDisk();
});

// PDF template catalog for the export dialog (and whoever else wants it)
ipcMain.handle('get-pdf-templates', () => {
    const manifest = getPdfTemplatesManifest();
    return Object.entries(manifest).map(([key, meta]) => ({
        key,
        name: meta.name || key,
        description: meta.description || '',
        supportsTitlePage: meta.supportsTitlePage !== false,
        titlePageFields: meta.titlePageFields || []
    }));
});

// Recent files handlers
ipcMain.handle('get-recent-files', () => {
    return recentFiles;
});

function addToRecentFiles(filePath) {
    const index = recentFiles.indexOf(filePath);
    if (index > -1) {
        recentFiles.splice(index, 1);
    }
    recentFiles.unshift(filePath);
    if (recentFiles.length > 10) {
        recentFiles.pop();
    }
    saveRecentFilesToDisk();
    updateRecentFilesMenu();
}

// Open a specific file by path (for recent files)
ipcMain.on('open-file-path', async (event, filePath) => {
    try {
        const content = await fs.readFile(filePath, 'utf-8');
        currentFilePath = filePath;
        lastSaveDirectory = path.dirname(filePath);
        documentEdited = false;
        event.reply('file-opened', { filePath, content });
        addToRecentFiles(filePath);
    } catch (error) {
        dialog.showErrorBox('Fehler', `Datei konnte nicht geöffnet werden: ${error.message}`);
    }
});

// Directory listing for sidebar file tree
ipcMain.handle('list-directory', async (event, dirPath) => {
    try {
        const entries = await fs.readdir(dirPath, { withFileTypes: true });
        return entries
            .filter(e => !e.name.startsWith('.'))
            .map(e => ({
                name: e.name,
                path: path.join(dirPath, e.name),
                isDirectory: e.isDirectory()
            }))
            .sort((a, b) => {
                if (a.isDirectory !== b.isDirectory) return a.isDirectory ? -1 : 1;
                return a.name.localeCompare(b.name);
            });
    } catch (error) {
        console.error('Error listing directory:', error);
        return [];
    }
});

// Image file picker dialog
ipcMain.handle('select-image', async () => {
    const result = await dialog.showOpenDialog(mainWindow, {
        properties: ['openFile'],
        filters: [
            { name: 'Bilder', extensions: ['png', 'jpg', 'jpeg', 'gif', 'svg', 'webp', 'bmp'] }
        ]
    });
    if (!result.canceled && result.filePaths.length > 0) {
        return result.filePaths[0];
    }
    return null;
});

// A2: Electron dialog replacements for confirm/alert
ipcMain.handle('show-confirm-dialog', async (event, { message, detail, buttons, defaultId, cancelId }) => {
    const response = await dialog.showMessageBox(mainWindow, {
        type: 'question',
        buttons: buttons || ['OK', 'Abbrechen'],
        defaultId: defaultId || 0,
        cancelId: cancelId !== undefined ? cancelId : 1,
        message: message || '',
        detail: detail || undefined
    });
    return response.response;
});

ipcMain.handle('show-alert-dialog', async (event, { message, detail, type }) => {
    await dialog.showMessageBox(mainWindow, {
        type: type || 'info',
        buttons: ['OK'],
        message: message || '',
        detail: detail || undefined
    });
});

// File stats for info panel (B3)
ipcMain.handle('get-file-stats', async (event, filePath) => {
    try {
        const stat = await fs.stat(filePath);
        return {
            size: stat.size,
            created: stat.birthtime.toISOString(),
            modified: stat.mtime.toISOString()
        };
    } catch (error) {
        return null;
    }
});

// C2: Save pasted clipboard image to disk
ipcMain.handle('save-clipboard-image', async (event, { dirPath, buffer, filename }) => {
    try {
        const imagesDir = path.join(dirPath, 'images');
        try { await fs.mkdir(imagesDir, { recursive: true }); } catch (e) { /* exists */ }
        const filePath = path.join(imagesDir, filename);
        await fs.writeFile(filePath, Buffer.from(buffer));
        return `images/${filename}`;
    } catch (error) {
        console.error('Error saving clipboard image:', error);
        return null;
    }
});

// C7: Copy dropped image file to images/ subfolder
ipcMain.handle('copy-image-file', async (event, { sourcePath, dirPath }) => {
    try {
        const imagesDir = path.join(dirPath, 'images');
        try { await fs.mkdir(imagesDir, { recursive: true }); } catch (e) { /* exists */ }
        const fileName = path.basename(sourcePath);
        const destPath = path.join(imagesDir, fileName);
        await fs.copyFile(sourcePath, destPath);
        return `images/${fileName}`;
    } catch (error) {
        console.error('Error copying image file:', error);
        return null;
    }
});

// Session state persistence for crash recovery
const sessionPath = path.join(userDataPath, 'session.json');

ipcMain.on('save-session', async (event, sessionData) => {
    // During clean shutdown the beforeunload fires after clearSessionFile() — ignore it
    if (isCleanShutdown) {
        clearSessionFile();
        return;
    }
    try {
        await fs.writeFile(sessionPath, JSON.stringify(sessionData, null, 2), 'utf-8');
    } catch (error) {
        console.error('Error saving session:', error);
    }
});

ipcMain.handle('get-session', () => {
    try {
        if (fsSync.existsSync(sessionPath)) {
            const data = fsSync.readFileSync(sessionPath, 'utf-8');
            return JSON.parse(data);
        }
    } catch (error) {
        console.error('Error loading session:', error);
    }
    return null;
});

ipcMain.on('clear-session', () => {
    clearSessionFile();
});

function clearSessionFile() {
    try {
        if (fsSync.existsSync(sessionPath)) {
            fsSync.unlinkSync(sessionPath);
        }
    } catch (error) {
        console.error('Error clearing session:', error);
    }
}

// Shell operations — external links are confirmed per host (session-scoped allowlist).
// Protects against phishing/tracking embedded in untrusted markdown documents.
const trustedHosts = new Set();

ipcMain.on('open-external', async (event, url) => {
    if (typeof url !== 'string') return;
    if (!url.startsWith('https://') && !url.startsWith('http://')) return;

    let host;
    try {
        host = new URL(url).host;
    } catch {
        return; // malformed URL — drop silently
    }

    if (!trustedHosts.has(host)) {
        const result = await dialog.showMessageBox(mainWindow, {
            type: 'question',
            buttons: ['Öffnen', 'Abbrechen'],
            defaultId: 0,
            cancelId: 1,
            checkboxLabel: `Für diese Sitzung ${host} vertrauen`,
            checkboxChecked: false,
            title: 'Externen Link öffnen?',
            message: `Möchtest du diesen Link im Browser öffnen?`,
            detail: url
        });
        if (result.response !== 0) return;
        if (result.checkboxChecked) trustedHosts.add(host);
    }

    shell.openExternal(url);
});

// Window controls
ipcMain.on('update-window-title', (event, title) => {
    mainWindow.setTitle(title);
});

ipcMain.on('set-document-edited', (event, edited) => {
    documentEdited = edited;
    mainWindow.setDocumentEdited(edited);
});

// F2: File watching handlers — fs.watch (event-based, no polling)
ipcMain.on('watch-file', (event, filePath) => {
    if (!filePath || fsWatchers.has(filePath)) return;

    if (fsWatchers.size >= 50) {
        console.warn('File watcher limit reached (50), not adding:', filePath);
        return;
    }

    try {
        if (!fsSync.existsSync(filePath)) {
            console.error('File does not exist:', filePath);
            return;
        }

        let debounceTimer = null;
        const watcher = fsSync.watch(filePath, { persistent: true }, (eventType) => {
            if (debounceTimer) clearTimeout(debounceTimer);
            debounceTimer = setTimeout(() => {
                if (!fsSync.existsSync(filePath)) {
                    // File was deleted
                    if (mainWindow && !mainWindow.isDestroyed()) {
                        mainWindow.webContents.send('file-deleted-externally', { filePath });
                    }
                    watcher.close();
                    fsWatchers.delete(filePath);
                    return;
                }
                if (eventType === 'change') {
                    fs.readFile(filePath, 'utf-8').then(content => {
                        if (mainWindow && !mainWindow.isDestroyed()) {
                            mainWindow.webContents.send('file-changed-externally', { filePath, content });
                        }
                    }).catch(err => {
                        console.error('Error reading changed file:', err);
                    });
                }
            }, 200);
        });

        watcher.on('error', (err) => {
            console.error('File watcher error:', err);
            watcher.close();
            fsWatchers.delete(filePath);
        });

        fsWatchers.set(filePath, watcher);
    } catch (error) {
        console.error('Error setting up file watcher:', error);
    }
});

ipcMain.on('unwatch-file', (event, filePath) => {
    if (fsWatchers.has(filePath)) {
        const watcher = fsWatchers.get(filePath);
        if (watcher && watcher.close) {
            watcher.close();
        }
        fsWatchers.delete(filePath);
    }
});

// About Dialog
function showAboutDialog() {
    const aboutOptions = {
        type: 'info',
        title: 'Über MrxDown',
        message: 'MrxDown',
        detail: `Version ${packageJson.version}

Ein moderner Markdown-Editor mit Live-Vorschau

Entwickler: Martin Pfeffer © 2025

Open Source unter MIT-Lizenz
GitHub: https://github.com/pepperonas/mrxdown

Features:
• Live-Vorschau
• Tabs und Drag & Drop
• Syntax-Highlighting
• Export-Funktionen
• Tabellen-Editor

Entwickelt mit Electron`,
        buttons: ['OK', 'GitHub öffnen'],
        defaultId: 0,
        icon: path.join(__dirname, 'assets/icon.png')
    };

    dialog.showMessageBox(mainWindow, aboutOptions).then((result) => {
        if (result.response === 1) {
            // GitHub öffnen
            shell.openExternal('https://github.com/pepperonas/mrxdown');
        }
    });
}

// CLI Support - convert markdown to PDF from command line
async function runCLI(inputFile) {
    const marked = require('marked');
    const os = require('os');

    try {
        // Resolve absolute path
        const absolutePath = path.isAbsolute(inputFile)
            ? inputFile
            : path.resolve(process.cwd(), inputFile);

        // Check if file exists
        try {
            await fs.access(absolutePath);
        } catch {
            console.error(`Fehler: Datei nicht gefunden: ${absolutePath}`);
            app.exit(1);
            return;
        }

        // Check if it's a markdown file
        const ext = path.extname(absolutePath).toLowerCase();
        if (ext !== '.md' && ext !== '.markdown') {
            console.error(`Fehler: Keine Markdown-Datei: ${absolutePath}`);
            app.exit(1);
            return;
        }

        console.log(`Konvertiere: ${absolutePath}`);

        // Read markdown content
        const markdownContent = await fs.readFile(absolutePath, 'utf-8');

        // Extract frontmatter once — drives both KaTeX rendering and PDF template selection
        const { frontmatter, body: markdownBody } = extractFrontmatter(markdownContent);

        // D2: Render KaTeX server-side so CLI PDFs include math. Mermaid is GUI-only.
        const markdownWithMath = renderMathForCLI(markdownBody);

        // Convert to HTML
        const htmlContent = marked.parse(markdownWithMath);

        // Convert images to base64
        currentFilePath = absolutePath;
        const htmlWithImages = await convertImagesToBase64(htmlContent);

        // Create temporary HTML file (data URLs are too long for Electron)
        const tempHtmlPath = path.join(os.tmpdir(), `mrxdown-cli-${Date.now()}.html`);
        await fs.writeFile(tempHtmlPath, buildPdfHtml({ bodyContent: htmlWithImages, frontmatter }), 'utf-8');

        // Create hidden window for PDF generation
        const pdfWindow = new BrowserWindow({
            width: 800,
            height: 1000,
            show: false,
            webPreferences: {
                nodeIntegration: false,
                contextIsolation: true
            }
        });

        // Load HTML from temp file
        await pdfWindow.loadFile(tempHtmlPath);

        // Wait for content to load
        await new Promise(resolve => setTimeout(resolve, 1000));

        // Generate PDF
        const pdfData = await pdfWindow.webContents.printToPDF({
            marginsType: 0,
            pageSize: 'A4',
            printBackground: true,
            landscape: false,
            preferCSSPageSize: true
        });

        pdfWindow.close();

        // Clean up temp file
        await fs.unlink(tempHtmlPath).catch(() => {});

        // Save PDF with same name as input file
        const outputPath = absolutePath.replace(/\.(md|markdown)$/i, '.pdf');
        await fs.writeFile(outputPath, pdfData);

        console.log(`PDF erstellt: ${outputPath}`);
        app.exit(0);

    } catch (error) {
        console.error(`Fehler bei der Konvertierung: ${error.message}`);
        app.exit(1);
    }
}

// CLI Batch mode - convert all markdown files in a directory
async function runCLIBatch(inputDir) {
    const marked = require('marked');
    const os = require('os');

    try {
        // Resolve absolute path
        const absolutePath = path.isAbsolute(inputDir)
            ? inputDir
            : path.resolve(process.cwd(), inputDir);

        // Check if directory exists
        try {
            const stat = await fs.stat(absolutePath);
            if (!stat.isDirectory()) {
                console.error(`Fehler: Kein Verzeichnis: ${absolutePath}`);
                app.exit(1);
                return;
            }
        } catch {
            console.error(`Fehler: Verzeichnis nicht gefunden: ${absolutePath}`);
            app.exit(1);
            return;
        }

        // Find all markdown files
        const files = await fs.readdir(absolutePath);
        const mdFiles = files.filter(f => f.endsWith('.md') || f.endsWith('.markdown'));

        if (mdFiles.length === 0) {
            console.log(`Keine Markdown-Dateien in ${absolutePath} gefunden.`);
            app.exit(0);
            return;
        }

        console.log(`Gefunden: ${mdFiles.length} Markdown-Datei(en) in ${absolutePath}`);

        let successCount = 0;
        let errorCount = 0;

        for (const mdFile of mdFiles) {
            const filePath = path.join(absolutePath, mdFile);
            try {
                console.log(`Konvertiere: ${mdFile}`);

                // Read markdown content
                const markdownContent = await fs.readFile(filePath, 'utf-8');

                // Extract frontmatter — drives KaTeX + PDF template choice
                const { frontmatter, body: markdownBody } = extractFrontmatter(markdownContent);

                // D2: Server-side KaTeX for CLI batch mode (same as single-file CLI)
                const markdownWithMath = renderMathForCLI(markdownBody);

                // Convert to HTML
                const htmlContent = marked.parse(markdownWithMath);

                // Convert images to base64
                currentFilePath = filePath;
                const htmlWithImages = await convertImagesToBase64(htmlContent);

                // Create temporary HTML file
                const tempHtmlPath = path.join(os.tmpdir(), `mrxdown-cli-${Date.now()}.html`);
                await fs.writeFile(tempHtmlPath, buildPdfHtml({ bodyContent: htmlWithImages, frontmatter }), 'utf-8');

                // Create hidden window for PDF generation
                const pdfWindow = new BrowserWindow({
                    width: 800,
                    height: 1000,
                    show: false,
                    webPreferences: { nodeIntegration: false, contextIsolation: true }
                });

                await pdfWindow.loadFile(tempHtmlPath);
                await new Promise(resolve => setTimeout(resolve, 500));

                const pdfData = await pdfWindow.webContents.printToPDF({
                    marginsType: 0,
                    pageSize: 'A4',
                    printBackground: true,
                    landscape: false,
                    preferCSSPageSize: true
                });

                pdfWindow.close();
                await fs.unlink(tempHtmlPath).catch(() => {});

                const outputPath = filePath.replace(/\.(md|markdown)$/i, '.pdf');
                await fs.writeFile(outputPath, pdfData);

                console.log(`  ✓ ${mdFile.replace(/\.(md|markdown)$/i, '.pdf')}`);
                successCount++;

            } catch (error) {
                console.error(`  ✗ ${mdFile}: ${error.message}`);
                errorCount++;
            }
        }

        console.log(`\nFertig: ${successCount} erfolgreich, ${errorCount} fehlgeschlagen`);
        app.exit(errorCount > 0 ? 1 : 0);

    } catch (error) {
        console.error(`Fehler: ${error.message}`);
        app.exit(1);
    }
}

// Check for CLI arguments
// In packaged mode process.argv has no script path at [1], so user args start at [1].
// In dev mode (electron main.js) they start at [2].
const args = process.argv.slice(app.isPackaged ? 1 : 2);
// --pdf flag explicitly requests PDF conversion (used by context menu and mrxdown.cmd wrapper)
const hasPdfFlag = args.includes('--pdf');
const cliArg = args.find(arg => !arg.startsWith('-'));

// Determine whether this invocation is headless PDF conversion or GUI mode
function isHeadlessMode() {
    if (!cliArg) return false;
    if (hasPdfFlag) return true;
    try {
        return fsSync.statSync(path.isAbsolute(cliArg) ? cliArg : path.resolve(process.cwd(), cliArg)).isDirectory();
    } catch { return false; }
}

if (isHeadlessMode()) {
    // Headless PDF conversion — skip single-instance lock so it always runs
    app.whenReady().then(() => {
        if (fsSync.existsSync(path.isAbsolute(cliArg) ? cliArg : path.resolve(process.cwd(), cliArg)) &&
            fsSync.statSync(path.isAbsolute(cliArg) ? cliArg : path.resolve(process.cwd(), cliArg)).isDirectory()) {
            runCLIBatch(cliArg);
        } else {
            runCLI(cliArg);
        }
    });
} else {
    // GUI mode — enforce single instance so file-opens go to the running window
    const gotSingleInstanceLock = app.requestSingleInstanceLock();
    if (!gotSingleInstanceLock) {
        app.quit();
    } else {
        app.on('second-instance', (event, commandLine, workingDirectory) => {
            if (mainWindow) {
                if (mainWindow.isMinimized()) mainWindow.restore();
                mainWindow.focus();
            }
            // Open the file passed to the second instance in the already-running editor.
            // Same argv-slicing convention as startup: packaged builds don't have the script path at [1].
            // Filter for markdown/text files explicitly instead of "first non-flag arg" to skip
            // any injected Electron flags (--enable-features=..., --enable-logging, etc.).
            const sliceStart = app.isPackaged ? 1 : 2;
            const secondArg = commandLine.slice(sliceStart).find(a => !a.startsWith('-') && /\.(md|markdown|txt)$/i.test(a));
            if (secondArg && mainWindow && !commandLine.includes('--pdf')) {
                const absPath = path.isAbsolute(secondArg) ? secondArg : path.resolve(workingDirectory, secondArg);
                fs.readFile(absPath, 'utf-8').then(content => {
                    mainWindow.webContents.send('file-opened', { filePath: absPath, content });
                    addToRecentFiles(absPath);
                }).catch(err => console.error('Error opening file from second instance:', err));
            }
        });

        if (cliArg) {
            // GUI mode with a file argument (file association / double-click)
            const absolutePath = path.isAbsolute(cliArg) ? cliArg : path.resolve(process.cwd(), cliArg);
            pendingOpenFile = absolutePath;
        }
        app.whenReady().then(() => {
            createWindow();
            // Prune missing recent files async — deferred so offline network drives don't block startup
            setTimeout(() => pruneMissingRecentFiles(), 1000);
            // Auto-update check: only in packaged builds, deferred so the window is interactive first.
            // Dev builds can't update themselves; CLI mode is already excluded by the surrounding else-branch.
            if (app.isPackaged) {
                setTimeout(() => initAutoUpdater().catch(err => console.warn('AutoUpdater init failed:', err.message)), 3000);
            }
        });
    }
}

// --- Auto-Updater Integration (electron-updater) ---
// Lazy-loaded so dev mode doesn't pull the dependency at startup.
let _autoUpdater = null;
let _autoUpdaterReady = false;

async function initAutoUpdater() {
    if (_autoUpdaterReady) return _autoUpdater;
    try {
        const mod = require('electron-updater');
        _autoUpdater = mod.autoUpdater;
    } catch (err) {
        console.warn('electron-updater not available:', err.message);
        return null;
    }

    _autoUpdater.autoDownload = true;
    _autoUpdater.autoInstallOnAppQuit = true;

    _autoUpdater.on('checking-for-update', () => console.log('[updater] checking…'));
    _autoUpdater.on('update-available', (info) => {
        console.log('[updater] update available:', info && info.version);
        if (mainWindow) mainWindow.webContents.send('updater-status', { state: 'available', version: info && info.version });
    });
    _autoUpdater.on('update-not-available', () => console.log('[updater] up to date'));
    _autoUpdater.on('error', (err) => console.warn('[updater] error:', err && err.message));
    _autoUpdater.on('download-progress', (p) => {
        if (mainWindow) mainWindow.webContents.send('updater-status', {
            state: 'downloading',
            percent: Math.round(p.percent),
            bytesPerSecond: p.bytesPerSecond
        });
    });
    _autoUpdater.on('update-downloaded', async (info) => {
        console.log('[updater] downloaded:', info && info.version);
        if (mainWindow) mainWindow.webContents.send('updater-status', { state: 'downloaded', version: info && info.version });
        const choice = await dialog.showMessageBox(mainWindow, {
            type: 'info',
            buttons: ['Jetzt neu starten', 'Später'],
            defaultId: 0,
            cancelId: 1,
            title: 'Update bereit',
            message: `MrxDown ${info && info.version} ist heruntergeladen.`,
            detail: 'Beim Neustart wird die neue Version installiert.'
        });
        if (choice.response === 0) {
            _autoUpdater.quitAndInstall();
        }
    });

    _autoUpdaterReady = true;
    try {
        await _autoUpdater.checkForUpdates();
    } catch (err) {
        console.warn('[updater] check failed:', err.message);
    }
    return _autoUpdater;
}

// Manual update check from the menu. Shows a dialog with the result instead of
// silently hoping the auto-download fires; user-initiated checks expect feedback.
async function triggerManualUpdateCheck() {
    if (!app.isPackaged) {
        dialog.showMessageBox(mainWindow, {
            type: 'info',
            title: 'Update-Prüfung',
            message: 'Dev-Build',
            detail: 'Auto-Updates sind nur in installierten Builds verfügbar.'
        });
        return;
    }
    const upd = await initAutoUpdater();
    if (!upd) {
        dialog.showErrorBox('Update-Prüfung', 'Auto-Updater nicht verfügbar.');
        return;
    }
    try {
        const result = await upd.checkForUpdates();
        if (!result || !result.updateInfo || result.updateInfo.version === packageJson.version) {
            dialog.showMessageBox(mainWindow, {
                type: 'info',
                title: 'Update-Prüfung',
                message: `Du hast bereits die neueste Version (${packageJson.version}).`
            });
        }
        // If there IS an update, the auto-download + downloaded handler takes over.
    } catch (err) {
        dialog.showErrorBox('Update-Prüfung fehlgeschlagen', err.message);
    }
}

// Manual "check for updates" trigger (wired to menu)
ipcMain.handle('check-for-updates', async () => {
    if (!app.isPackaged) {
        return { ok: false, reason: 'dev-build' };
    }
    const upd = await initAutoUpdater();
    if (!upd) return { ok: false, reason: 'updater-unavailable' };
    try {
        const result = await upd.checkForUpdates();
        return { ok: true, updateInfo: result && result.updateInfo };
    } catch (err) {
        return { ok: false, reason: 'check-failed', error: err.message };
    }
});

// A7: macOS file association — handle open-file before and after ready
app.on('open-file', (event, filePath) => {
    event.preventDefault();
    if (mainWindow && !mainWindow.isDestroyed()) {
        // App is running — open file in new tab
        mainWindow.webContents.send('file-opened-external', { filePath });
        fs.readFile(filePath, 'utf-8').then(content => {
            mainWindow.webContents.send('file-opened', { filePath, content });
            addToRecentFiles(filePath);
        }).catch(err => console.error('Error opening file:', err));
    } else {
        // App not ready yet — remember for later
        pendingOpenFile = filePath;
    }
});

// A3: Crash handlers
process.on('uncaughtException', (error) => {
    console.error('Uncaught exception:', error);
    if (mainWindow && !mainWindow.isDestroyed()) {
        dialog.showErrorBox('Unerwarteter Fehler', `${error.message}\n\nDie App versucht weiterzulaufen.`);
    }
});

app.on('render-process-gone', (event, webContents, details) => {
    console.error('Render process gone:', details.reason);
    if (details.reason !== 'clean-exit') {
        // Recreate window on crash
        createWindow();
    }
});

app.on('window-all-closed', () => {
    if (process.platform !== 'darwin') {
        app.quit();
    }
});

app.on('activate', () => {
    if (mainWindow === null) {
        createWindow();
    }
});

// Simple test to verify file watching works
if (process.env.NODE_ENV === 'development') {
    app.whenReady().then(() => {
        console.log('App is ready, file watching system initialized');
    });
}