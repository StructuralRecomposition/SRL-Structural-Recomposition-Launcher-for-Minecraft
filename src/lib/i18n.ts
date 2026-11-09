import { EN } from "./i18n.en";

export type Language = "zh-CN" | "en";
export const DEFAULT_LANGUAGE: Language = "zh-CN";
const STORAGE_KEY = "srl.language";

let current: Language = DEFAULT_LANGUAGE;

function readStored(): Language | null {
  try {
    const value = globalThis.localStorage?.getItem(STORAGE_KEY);
    return value === "en" || value === "zh-CN" ? value : null;
  } catch {
    return null;
  }
}

current = readStored() ?? DEFAULT_LANGUAGE;

export function getLanguage(): Language {
  return current;
}

export function normalizeLanguage(value: string | null | undefined): Language {
  return value === "en" ? "en" : DEFAULT_LANGUAGE;
}

export function setLanguage(value: Language): void {
  current = value;
  try {
    globalThis.localStorage?.setItem(STORAGE_KEY, value);
  } catch {
    // Storage can be unavailable in private windows; the in-memory value still
    // applies for this session.
  }
  if (typeof document !== "undefined") {
    document.documentElement.lang = value === "en" ? "en" : "zh-CN";
  }
}

export function interpolate(template: string, params?: readonly unknown[]): string {
  if (!params?.length) return template;
  return template.replace(/\{(\d+)\}/g, (match, index) => {
    const value = params[Number(index)];
    return value === undefined || value === null ? match : String(value);
  });
}

/**
 * Translate a Chinese source string. Missing entries intentionally fall back
 * to the Chinese source instead of showing a raw key.
 */
export function t(zh: string, params?: readonly unknown[]): string {
  const text = current === "en" ? (EN[zh] ?? zh) : zh;
  return interpolate(text, params);
}

/**
 * Best-effort translation for strings that originate in Rust commands. Exact
 * matches are cheap; dynamic messages keep their Chinese fallback when no
 * pattern is known.
 */
export function translateMessage(message: string): string {
  if (current !== "en" || !message) return message;
  const exact = EN[message];
  if (exact) return exact;
  for (const [pattern, replacement] of MESSAGE_PATTERNS) {
    if (pattern.test(message)) return message.replace(pattern, replacement);
  }
  return message;
}

const MESSAGE_PATTERNS: Array<[RegExp, string]> = [
  [/^服务器返回 HTTP (\d+)$/, "Server returned HTTP $1"],
  [/^CurseForge 返回 HTTP (\d+)$/, "CurseForge returned HTTP $1"],
  [/^下载文件大小与元数据不一致$/, "Downloaded file size does not match metadata"],
  [/^网络请求超时$/, "Network request timed out"],
  [/^连接超时$/, "Connection timed out"],
  [/^下载失败$/, "Download failed"],
];

export function errorText(error: unknown): string {
  const raw = error instanceof Error ? error.message : String(error);
  return translateMessage(raw);
}
