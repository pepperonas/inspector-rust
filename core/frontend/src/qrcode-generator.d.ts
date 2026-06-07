// Minimal ambient types for the dependency-free `qrcode-generator` package
// (Kazuhiko Arase's classic encoder) — it ships no .d.ts.
declare module "qrcode-generator" {
  interface QRCode {
    addData(data: string): void;
    make(): void;
    getModuleCount(): number;
    isDark(row: number, col: number): boolean;
    createDataURL(cellSize?: number, margin?: number): string;
  }
  type ErrorCorrectionLevel = "L" | "M" | "Q" | "H";
  function qrcode(typeNumber: number, errorCorrectionLevel: ErrorCorrectionLevel): QRCode;
  export default qrcode;
}
