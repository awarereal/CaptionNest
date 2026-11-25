import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { ensureFonts, type CaptionStyle } from "../lib/captionStyle";
import { drawCaption } from "../lib/renderCaption";

/** Size of the video picture inside a box, as `object-fit: contain` places it. */
export function fitRect(boxW: number, boxH: number, aspect: number): { w: number; h: number } {
  if (boxW <= 0 || boxH <= 0 || !(aspect > 0)) return { w: 0, h: 0 };
  return boxW / boxH > aspect ? { w: boxH * aspect, h: boxH } : { w: boxW, h: boxW / aspect };
}

/**
 * The active caption drawn over the video exactly as an exported video will show it:
 * same renderer, same proportions, at the screen's pixel density.
 */
export function CaptionCanvas({ text, style, aspect }: { text: string | null; style: CaptionStyle; aspect: number }) {
  const layerRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [box, setBox] = useState({ w: 0, h: 0 });

  useLayoutEffect(() => {
    const el = layerRef.current;
    if (!el) return;
    const measure = () => setBox({ w: el.clientWidth, h: el.clientHeight });
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const fit = fitRect(box.w, box.h, aspect);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    let alive = true;
    const dpr = window.devicePixelRatio || 1;
    const w = Math.max(1, Math.round(fit.w * dpr));
    const h = Math.max(1, Math.round(fit.h * dpr));
    const draw = () => {
      if (!alive) return;
      if (canvas.width !== w) canvas.width = w;
      if (canvas.height !== h) canvas.height = h;
      const ctx = canvas.getContext("2d");
      if (!ctx) return;
      ctx.clearRect(0, 0, w, h);
      if (text) drawCaption(ctx, text, style, w, h);
    };
    draw();
    // Faces load on demand: draw again once the ones this caption needs are in.
    if (text) void ensureFonts(style, text).then(draw);
    return () => {
      alive = false;
    };
  }, [text, style, fit.w, fit.h]);

  return (
    <div ref={layerRef} className="cap-layer" aria-hidden>
      <canvas ref={canvasRef} className="cap-layer__canvas" style={{ width: fit.w, height: fit.h }} />
    </div>
  );
}
