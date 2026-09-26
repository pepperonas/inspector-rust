/**
 * The slim history list (`db::list_slim`) no longer ships `content_data` for
 * TEXT rows — for almost every text clip it is byte-identical to
 * `content_text`, so sending both doubled the decrypt + IPC cost of every
 * popup open. The one exception: a few writers store a SHORT preview in
 * `content_text` (e.g. generated pastes keep the first 200 chars) and the
 * full text only in `content_data`. This decides whether the preview has to
 * fetch the full row by id.
 *
 * `byte_size` is the UTF-8 length of the full payload, so a `content_text`
 * whose UTF-8 length is smaller is a truncated preview.
 */
export function needsFullText(inlineData: string, contentText: string, byteSize: number): boolean {
  if (inlineData) return false; // the row already carries the payload
  return new TextEncoder().encode(contentText).length < byteSize;
}
