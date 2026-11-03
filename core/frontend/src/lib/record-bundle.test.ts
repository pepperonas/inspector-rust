import { describe, expect, it } from "vitest";

/** Reads a repo file (the frontend tests run with `core/frontend` as cwd). */
async function repoFile(path: string): Promise<string> {
  const { readFileSync } = (await import("node:" + "fs")) as unknown as {
    readFileSync(path: string, encoding: "utf8"): string;
  };
  const cwd = (globalThis as unknown as { process: { cwd(): string } }).process.cwd();
  return readFileSync(`${cwd}/../../${path}`, "utf8");
}

describe("record webcam: the bundle may ask for the camera", () => {
  // Without the usage string, TCC ends the process the moment the camera is requested;
  // without the entitlement, the hardened runtime denies it before any prompt.
  it.each(["scripts/install-macos.sh", "scripts/pack-macos-release-dmg.sh"])(
    "%s injects NSCameraUsageDescription with plutil",
    async (script) => {
      const src = await repoFile(script);
      expect(src).toMatch(/plutil -replace NSCameraUsageDescription -string/);
    },
  );

  it("the entitlements carry com.apple.security.device.camera = true", async () => {
    const src = await repoFile("macos/src-tauri/entitlements.plist");
    expect(src).toMatch(/<key>com\.apple\.security\.device\.camera<\/key>\s*<true\/>/);
  });
});
