import type { CSSProperties } from "react";

export type IconName =
  | "home"
  | "box"
  | "layers"
  | "download"
  | "coffee"
  | "user"
  | "settings"
  | "activity"
  | "plug"
  | "play"
  | "stop"
  | "plus"
  | "refresh"
  | "trash"
  | "copy"
  | "check"
  | "alert"
  | "close"
  | "chevron"
  | "terminal"
  | "moon"
  | "sun"
  | "search"
  | "folder"
  | "shield"
  | "cpu"
  | "info"
  | "edit"
  | "map"
  | "palette";

const paths: Record<IconName, string> = {
  home: "M3 10.5 12 3l9 7.5M5 9.5V21h5v-6h4v6h5V9.5",
  box: "M21 8.5 12 3 3 8.5v7L12 21l9-5.5v-7ZM3.3 8.7 12 13.6l8.7-4.9M12 13.6V21",
  layers: "m12 3 9 5-9 5-9-5 9-5Zm9 9-9 5-9-5m18 4-9 5-9-5",
  download: "M12 3v12m0 0 4.5-4.5M12 15l-4.5-4.5M4 19.5h16",
  coffee: "M4 9h13v6a5 5 0 0 1-5 5H9a5 5 0 0 1-5-5V9Zm13 1h2a3 3 0 0 1 0 6h-2M7 3v3m4-3v3m4-3v3",
  user: "M12 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8Zm-8 9a8 8 0 0 1 16 0",
  settings:
    "M12 15.5a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7Zm8-3.5c0-.5 0-1-.1-1.4l2-1.6-2-3.4-2.4 1a8.4 8.4 0 0 0-2.4-1.4L14.7 2h-4l-.4 2.6c-.9.3-1.7.8-2.4 1.4l-2.4-1-2 3.4 2 1.6a9 9 0 0 0 0 2.8l-2 1.6 2 3.4 2.4-1c.7.6 1.5 1.1 2.4 1.4l.4 2.8h4l.4-2.6a8.4 8.4 0 0 0 2.4-1.4l2.4 1 2-3.4-2-1.6c.1-.4.1-.9.1-1.4Z",
  activity: "M3 12h4l3-8 4 16 3-8h4",
  plug: "M9 3v6m6-6v6M7 9h10v3a5 5 0 0 1-10 0V9Zm5 8v4",
  play: "M8 5.5v13l11-6.5-11-6.5Z",
  stop: "M6 6h12v12H6z",
  plus: "M12 5v14M5 12h14",
  refresh: "M20 11a8 8 0 1 0-2.3 6.7M20 5v6h-6",
  trash: "M4 7h16M9 7V4h6v3m-8 0 1 14h8l1-14M10 11v6m4-6v6",
  copy: "M9 9h11v11H9V9Zm-1 6H4V4h11v4",
  check: "m4 12 5 5L20 6",
  alert: "M12 3 2.5 20h19L12 3Zm0 6v5m0 3v.5",
  close: "m6 6 12 12M18 6 6 18",
  chevron: "m8 10 4 4 4-4",
  terminal: "m4 5h16v14H4V5Zm3 4 3 3-3 3m5 1h5",
  moon: "M20 14.5A8.5 8.5 0 0 1 9.5 4a8.5 8.5 0 1 0 10.5 10.5Z",
  sun: "M12 17a5 5 0 1 0 0-10 5 5 0 0 0 0 10Zm0-14v2m0 18v-2M3 12h2m14 0h2M5.6 5.6 7 7m10 10 1.4 1.4M5.6 18.4 7 17m10-10 1.4-1.4",
  search: "M10.5 18a7.5 7.5 0 1 0 0-15 7.5 7.5 0 0 0 0 15Zm5.5-1.5L21 21",
  folder: "M3 6h6l2 2h10v11H3V6Z",
  shield: "M12 3 4.5 6v5.5c0 4.6 3.2 8.4 7.5 9.5 4.3-1.1 7.5-4.9 7.5-9.5V6L12 3Zm-3 8.5 2.5 2.5 4.5-4.5",
  cpu: "M8 8h8v8H8V8Zm-3 3H3m2 5H3m2-9H3m18 4h-2m2 5h-2m2-9h-2M8 5V3m4 2V3m4 2V3M8 21v-2m4 2v-2m4 2v-2",
  info: "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18Zm0-13v.5m0 3v6",
  edit: "M4 20h4l11-11-4-4L4 16v4Zm9-15 4 4",
  map: "M3 6.5 9 3l6 3 6-3v14.5L15 21l-6-3-6 3V6.5ZM9 3v15m6-12v15",
  palette:
    "M12 3a9 9 0 1 0 0 18h1.2a2.3 2.3 0 0 0 0-4.6h-1a1.7 1.7 0 0 1 0-3.4h3.1A4.7 4.7 0 0 0 20 8.3C20 5.4 16.4 3 12 3ZM7.5 8h.01M10 6h.01M14 6h.01M6.5 11h.01",
};

interface IconProps {
  name: IconName;
  size?: number;
  className?: string;
  style?: CSSProperties;
  filled?: boolean;
}

export function Icon({ name, size = 18, className, style, filled }: IconProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill={filled ? "currentColor" : "none"}
      stroke={filled ? "none" : "currentColor"}
      strokeWidth={1.8}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      style={style}
      aria-hidden="true"
      focusable="false"
    >
      <path d={paths[name]} />
    </svg>
  );
}
