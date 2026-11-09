import { afterEach, describe, expect, it } from "vitest";
import { interpolate, setLanguage, t, translateMessage } from "../lib/i18n";

afterEach(() => setLanguage("zh-CN"));

describe("i18n", () => {
  it("keeps Chinese by default", () => {
    expect(t("取消")).toBe("取消");
    expect(interpolate("{0} 个文件", [3])).toBe("3 个文件");
  });

  it("switches to English and interpolates", () => {
    setLanguage("en");
    expect(t("取消")).toBe("Cancel");
    expect(t("{0} 分钟前", [5])).toBe("5 minutes ago");
  });

  it("falls back to the source string for missing keys", () => {
    setLanguage("en");
    expect(t("这条文案还没有英文")).toBe("这条文案还没有英文");
  });

  it("translates exact backend messages", () => {
    setLanguage("en");
    expect(translateMessage("网络请求超时")).toBe("Network request timed out");
    expect(translateMessage("服务器返回 HTTP 503")).toBe("Server returned HTTP 503");
  });
});
