import { describe, expect, it } from "vitest";
import { formatBytes, formatEta, formatSpeed, percent, splitLines } from "../lib/format";

describe("format helpers", () => {
  it("formats byte sizes", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1024)).toBe("1.00 KB");
    expect(formatBytes(1024 * 1024 * 12.5)).toBe("12.5 MB");
    expect(formatBytes(1024 ** 3)).toBe("1.00 GB");
  });

  it("does not invent a speed when data is zero", () => {
    expect(formatSpeed(0)).toBe("—");
    expect(formatSpeed(2048)).toBe("2.00 KB/s");
  });

  it("only shows ETA when it is known", () => {
    expect(formatEta(null)).toBe("—");
    expect(formatEta(0)).toBe("—");
    expect(formatEta(42)).toBe("42 秒");
    expect(formatEta(125)).toBe("2 分 5 秒");
  });

  it("returns null percent for unknown totals", () => {
    expect(percent(10, 0)).toBeNull();
    expect(percent(50, 100)).toBe(50);
    expect(percent(200, 100)).toBe(100);
  });

  it("splits textarea arguments and drops blanks", () => {
    expect(splitLines("-Xmx2G\n\n-Dfoo=bar \n")).toEqual(["-Xmx2G", "-Dfoo=bar"]);
  });
});
