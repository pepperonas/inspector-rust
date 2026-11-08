# Screenshot editor — stage 2a: text-aware redaction and highlighting

Date: 2026-11-08. Follows the stage-1 design (`2026-11-08-screenshot-editor-design.md`).
The user asked me to choose the order and decide ("mach wie du empfiehlst"), so every
decision below is my call.

## Order of stage 2
1. **This document:** automatic redaction of sensitive text + a text-snapping
   highlighter/redactor. Both need the same thing: macOS text recognition with the
   position of every word.
2. Crop aspect presets, rotate/flip (small).
3. Second image on the canvas; reopen a screenshot for editing (bigger).

## Why
Screenshots get shared. The most common mistake is leaving an e-mail address, an
API key, an IBAN or a phone number visible. Drawing black boxes by hand over each
one is slow and easy to miss. CleanShot X has a "smart highlighter"; it does not
document automatic detection of secrets — that is where we go further.

## Goals
- **G1 Word positions from text recognition.** The `ir-ocr` helper gets a second
  request that returns every recognised line with its text and the box of every
  word (whitespace token), in source-image coordinates. Vision only — macOS.
- **G2 "Schwärzen: sensible Daten"** button in the editor. Finds, in the source
  image: e-mail addresses, phone numbers, IBANs, credit-card numbers (Luhn-checked),
  API keys / tokens (well-known prefixes and long high-entropy strings), IPv4
  addresses, and URLs carrying a token/key/password query parameter. Adds one
  redact block per finding (multi-word findings: one block per line), selects them
  all, one undo step. A finding already covered by a redact block is skipped, so
  pressing it twice adds nothing. A short message says what was found
  ("3 Stellen: 2 E-Mail, 1 Telefon") or that nothing was found.
- **G3 Snap to text.** With the highlighter, a drag that covers
  words becomes boxes tight around those words, one per line (one undo step). A
  drag that covers no word stays a plain rectangle, as today. Holding ⌥ while
  releasing keeps the plain rectangle. **Redactions never snap** (changed after a
  security review): a box fitted to the words would uncover anything else the drag
  covered — a signature, a photo, a QR code — without the user noticing.
- Recognition starts in the background when the editor opens, so snapping and the
  button are instant afterwards; while it runs, the button says it is reading.

## Non-goals
- Windows/Linux text positions (the button and snapping simply don't appear).
- Choosing which findings to redact in a list; the user deletes unwanted blocks
  (they are selected after insertion).
- Detecting names, addresses or faces.

## Anti-goals
- A finding must never be silently dropped because recognition failed — a failure
  is said, not shown as "nothing found".
- The helper's existing text request must behave exactly as before (main OCR path).
- No annotation chrome in exported images (unchanged).

## Constraints / costs
- Changing `ocr-helper` changes the `ir-ocr` binary: every user pays one cold
  model compile (~70 s) on the first recognition after this update. The existing
  background warm-up at launch absorbs it. Documented in CHANGELOG.
- Detection rules are pure and unit-tested; the helper's JSON encoder is tested.
- Word offsets are UTF-16 code units (what Vision's NSRange and JS strings use).
