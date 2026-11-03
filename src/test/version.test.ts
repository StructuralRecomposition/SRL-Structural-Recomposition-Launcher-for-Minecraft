import { describe, expect, it } from "vitest";
import type { InstalledVersion } from "../lib/api";
import {
  manifestTypeLabel,
  versionBucket,
  versionBucketLabel,
  versionBucketTone,
} from "../lib/version";
import { isExtension } from "../lib/files";
import { taskProgress } from "../lib/task";

function version(patch: Partial<InstalledVersion>): InstalledVersion {
  return {
    id: "1.20.4",
    versionType: "release",
    hasJson: true,
    hasJar: true,
    javaMajor: 17,
    mainClass: "net.minecraft.client.main.Main",
    sizeBytes: 100,
    modifiedAt: "2026-10-23T10:00:00Z",
    inheritsFrom: null,
    loader: null,
    path: "/tmp/versions/1.20.4",
    ...patch,
  };
}

describe("version categories", () => {
  it("separates releases from snapshots and legacy builds", () => {
    expect(versionBucket(version({}))).toBe("release");
    expect(versionBucket(version({ id: "26.4-snapshot-5", versionType: "snapshot" }))).toBe(
      "snapshot",
    );
    expect(versionBucket(version({ id: "b1.7.3", versionType: "old_beta" }))).toBe("old_beta");
    expect(versionBucket(version({ id: "a1.2.6", versionType: "old_alpha" }))).toBe("old_alpha");
  });

  it("treats loader builds as loaders even when the type says release", () => {
    const fabric = version({
      id: "fabric-loader-0.16.0-1.20.4",
      loader: "Fabric",
      inheritsFrom: "1.20.4",
    });
    expect(versionBucket(fabric)).toBe("loader");
    expect(versionBucketLabel(fabric)).toBe("Fabric");
    expect(versionBucketTone(fabric)).toBe("accent");
  });

  it("labels manifest types", () => {
    expect(manifestTypeLabel("release")).toBe("正式版");
    expect(manifestTypeLabel("snapshot")).toBe("快照");
    expect(manifestTypeLabel("old_beta")).toBe("旧版 Beta");
    expect(manifestTypeLabel("custom")).toBe("custom");
  });

  it("filters dropped files by extension", () => {
    expect(isExtension("/a/b.jar", [".jar"])).toBe(true);
    expect(isExtension("/a/b.JAR", [".jar"])).toBe(true);
    expect(isExtension("/a/b.zip.disabled", [".zip"])).toBe(false);
    expect(isExtension("/a/b.txt", [".jar", ".zip"])).toBe(false);
  });
});

describe("task progress", () => {
  it("does not show a full bar while files are still pending", () => {
    // Byte totals can be incomplete when loader metadata omits file sizes.
    expect(
      taskProgress({ doneBytes: 100, totalBytes: 100, doneFiles: 2, totalFiles: 5 }),
    ).toBeNull();
    expect(
      taskProgress({ doneBytes: 100, totalBytes: 100, doneFiles: 5, totalFiles: 5 }),
    ).toBe(100);
    expect(
      taskProgress({ doneBytes: 50, totalBytes: 100, doneFiles: 1, totalFiles: 5 }),
    ).toBe(50);
  });
});
