import { useEffect, useRef, useState } from "react";
import type { SkinModel } from "../lib/api";
import { t } from "../lib/i18n";


function useSkinImage(src?: string | null) {
  const [image, setImage] = useState<HTMLImageElement | null>(null);
  useEffect(() => {
    if (!src) {
      setImage(null);
      return;
    }
    let alive = true;
    const img = new Image();
    img.onload = () => {
      if (alive) setImage(img);
    };
    img.onerror = () => {
      if (alive) setImage(null);
    };
    img.src = src;
    return () => {
      alive = false;
    };
  }, [src]);
  return image;
}

function context(canvas: HTMLCanvasElement | null): CanvasRenderingContext2D | null {
  if (!canvas) return null;
  try {
    return canvas.getContext("2d");
  } catch {
    return null;
  }
}

function part(
  ctx: CanvasRenderingContext2D,
  image: HTMLImageElement,
  sx: number,
  sy: number,
  sw: number,
  sh: number,
  dx: number,
  dy: number,
  scale: number,
) {
  if (sw <= 0 || sh <= 0) return;
  ctx.drawImage(image, sx, sy, sw, sh, dx * scale, dy * scale, sw * scale, sh * scale);
}

/** Square head avatar cropped from the skin texture (hat layer included). */
export function SkinHead({
  src,
  size = 48,
  className = "",
}: {
  src?: string | null;
  size?: number;
  className?: string;
}) {
  const ref = useRef<HTMLCanvasElement>(null);
  const image = useSkinImage(src);
  useEffect(() => {
    const canvas = ref.current;
    const ctx = context(canvas);
    if (!canvas || !ctx) return;
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    ctx.imageSmoothingEnabled = false;
    if (!image) return;
    part(ctx, image, 8, 8, 8, 8, 0, 0, 8);
    part(ctx, image, 40, 8, 8, 8, 0, 0, 8);
  }, [image]);

  return (
    <canvas
      ref={ref}
      width={64}
      height={64}
      className={`skin-canvas ${className}`}
      style={{ width: size, height: size }}
      aria-hidden="true"
    />
  );
}

/**
 * Front-facing paper doll using the standard skin UV layout. The slim model
 * uses 3px arms; classic uses 4px. Overlay layers are drawn on top.
 */
export function SkinFigure({
  src,
  model,
  height = 168,
  className = "",
}: {
  src?: string | null;
  model: SkinModel;
  height?: number;
  className?: string;
}) {
  const ref = useRef<HTMLCanvasElement>(null);
  const image = useSkinImage(src);
  const slim = model === "slim";
  const armW = slim ? 3 : 4;
  const bodyX = slim ? 3 : 4;
  const totalW = slim ? 14 : 16;
  const scale = 6;

  useEffect(() => {
    const canvas = ref.current;
    const ctx = context(canvas);
    if (!canvas || !ctx) return;
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    ctx.imageSmoothingEnabled = false;
    if (!image) return;
    const armL = 0;
    const armR = bodyX + 8;
    // Base layers.
    part(ctx, image, 8, 8, 8, 8, bodyX, 0, scale);
    part(ctx, image, 20, 20, 8, 12, bodyX, 8, scale);
    part(ctx, image, 44, 20, armW, 12, armL, 8, scale);
    part(ctx, image, 36, 52, armW, 12, armR, 8, scale);
    part(ctx, image, 4, 20, 4, 12, bodyX, 20, scale);
    part(ctx, image, 20, 52, 4, 12, bodyX + 4, 20, scale);
    // Overlay layers (jacket, hat, sleeves, pants).
    part(ctx, image, 40, 8, 8, 8, bodyX, 0, scale);
    part(ctx, image, 20, 36, 8, 12, bodyX, 8, scale);
    part(ctx, image, 44, 36, armW, 12, armL, 8, scale);
    part(ctx, image, 52, 52, armW, 12, armR, 8, scale);
    part(ctx, image, 4, 36, 4, 12, bodyX, 20, scale);
    part(ctx, image, 4, 52, 4, 12, bodyX + 4, 20, scale);
  }, [image, armW, bodyX, scale]);

  return (
    <canvas
      ref={ref}
      width={totalW * scale}
      height={32 * scale}
      className={`skin-canvas ${className}`}
      style={{ height, width: "auto" }}
      aria-label={t("皮肤预览")}
    />
  );
}
