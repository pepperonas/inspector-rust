// Test helper: a transcription run with every action mocked (not shipped; only tests import it).
import { vi } from "vitest";
import type { Transcription } from "./useTranscription";

export const fakeTranscription = (over: Partial<Transcription> = {}): Transcription => ({
  running: false,
  progress: null,
  last: null,
  error: null,
  model: { id: "large-v3-turbo-q5", file: "m.bin", bytes: 574_041_195, installed: true },
  download: null,
  downloadError: null,
  start: vi.fn(),
  cancel: vi.fn(),
  downloadModel: vi.fn(),
  cancelDownload: vi.fn(),
  ...over,
});
