import { useEffect, useRef, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { Icon } from "./Icon";
import { isExtension } from "../lib/files";
import { isTauri } from "../lib/api";
import { t } from "../lib/i18n";


interface Props {
  accept: string[];
  onPaths: (paths: string[]) => void;
  title: string;
  hint: string;
  disabled?: boolean;
}

/**
 * A drop target that uses Tauri's native drag-drop event (absolute paths) and
 * falls back to nothing in the browser. Filtering happens before the callback,
 * so a dropped .txt never reaches the backend.
 */
export function DropZone({ accept, onPaths, title, hint, disabled }: Props) {
  const [over, setOver] = useState(false);
  const callback = useRef(onPaths);
  const acceptKey = accept.join(",");
  callback.current = onPaths;

  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    let disposed = false;
    const accepted = acceptKey.split(",").filter(Boolean);
    let webview: ReturnType<typeof getCurrentWebview>;
    try {
      webview = getCurrentWebview();
    } catch {
      // Browser/test environment without a Tauri webview: drop support is
      // simply unavailable, the picker buttons still work.
      return;
    }
    void webview
      .onDragDropEvent((event) => {
        if (disabled) return;
        if (event.payload.type === "enter") {
          setOver(true);
        } else if (event.payload.type === "leave") {
          setOver(false);
        } else if (event.payload.type === "drop") {
          setOver(false);
          const paths = event.payload.paths.filter((p) => isExtension(p, accepted));
          if (paths.length > 0) callback.current(paths);
        }
      })
      .then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [acceptKey, disabled]);

  return (
    <div className={`dropzone ${over ? "dropzone--over" : ""} ${disabled ? "dropzone--off" : ""}`}>
      <Icon name="download" size={22} />
      <div>
        <strong>{over ? t("松开以导入") : title}</strong>
        <p>{hint}</p>
      </div>
    </div>
  );
}
