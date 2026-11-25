// Draws one caption onto a canvas the size of a video frame. Used for the live preview
// (canvas over the player) and for burned-in exports (one PNG per caption), so what you
// see in CaptionNest is exactly what ends up in the video.
import { cssFont, type CaptionStyle } from "./captionStyle";

type Ctx = CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D;

/** Wide lines wrap at this share of the frame width. */
export const MAX_WIDTH = 0.88;

const RTL_CHAR = /[֐-ࣿיִ-﷿ﹰ-ﻼ]/;
const LETTER = /\p{L}/u;

/** Direction from the first letter, as a reader would see it. */
export function isRtlText(text: string): boolean {
  for (const ch of text) {
    if (LETTER.test(ch)) return RTL_CHAR.test(ch);
  }
  return false;
}

/** Splits text into lines no wider than `maxW`: kept line breaks first, then word wrap,
 *  then character wrap for scripts written without spaces. */
export function wrapLines(measure: (s: string) => number, text: string, maxW: number): string[] {
  const out: string[] = [];
  for (const para of text.split(/\n+/)) {
    let line = "";
    for (const word of para.trim().split(/\s+/).filter(Boolean)) {
      const cand = line ? `${line} ${word}` : word;
      if (measure(cand) <= maxW) {
        line = cand;
        continue;
      }
      if (line) out.push(line);
      if (measure(word) <= maxW) {
        line = word;
        continue;
      }
      // One "word" wider than the frame (e.g. Chinese or Japanese): break by character.
      line = "";
      for (const ch of Array.from(word)) {
        if (line && measure(line + ch) > maxW) {
          out.push(line);
          line = ch;
        } else line += ch;
      }
    }
    if (line) out.push(line);
  }
  return out;
}

function rgba(hex: string, alpha: number): string {
  const n = parseInt(hex.slice(1), 16);
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${alpha})`;
}

function roundRect(ctx: Ctx, x: number, y: number, w: number, h: number, r: number) {
  const rr = Math.min(r, h / 2, w / 2);
  ctx.beginPath();
  ctx.moveTo(x + rr, y);
  ctx.arcTo(x + w, y, x + w, y + h, rr);
  ctx.arcTo(x + w, y + h, x, y + h, rr);
  ctx.arcTo(x, y + h, x, y, rr);
  ctx.arcTo(x, y, x + w, y, rr);
  ctx.closePath();
}

/** Draws `text` styled as `style` on a `width` × `height` frame. */
export function drawCaption(ctx: Ctx, text: string, style: CaptionStyle, width: number, height: number) {
  const t = (style.uppercase ? text.toLocaleUpperCase() : text).trim();
  if (!t || width <= 0 || height <= 0) return;
  const px = Math.max(6, (style.size / 100) * Math.min(width, height));
  ctx.save();
  ctx.font = cssFont(style, px);
  ctx.textAlign = "center";
  ctx.textBaseline = "alphabetic";

  const lines = wrapLines((s) => ctx.measureText(s).width, t, width * MAX_WIDTH);
  // Line height from the font's own box: Bengali needs room for marks above and below.
  const m = ctx.measureText("Hgআকু");
  const ascent = m.fontBoundingBoxAscent || px * 0.95;
  const descent = m.fontBoundingBoxDescent || px * 0.3;
  const lineH = (ascent + descent) * 1.02;
  const padX = px * 0.42;
  const padY = px * 0.1;
  const gap = style.box ? padY * 2 + px * 0.06 : px * 0.02;
  const blockH = lines.length * lineH + (lines.length - 1) * gap;
  const margin = (style.margin / 100) * height;
  let top = style.position === "top" ? margin : style.position === "middle" ? (height - blockH) / 2 : height - margin - blockH;
  top = Math.max(padY, Math.min(height - blockH - padY, top));

  const cx = width / 2;
  const outline = (style.outline / 100) * px;
  ctx.lineJoin = "round";
  ctx.miterLimit = 2;
  lines.forEach((line, i) => {
    const y = top + i * (lineH + gap);
    const baseline = y + (lineH - (ascent + descent)) / 2 + ascent;
    ctx.direction = isRtlText(line) ? "rtl" : "ltr";
    if (style.box && style.boxOpacity > 0) {
      const w = ctx.measureText(line).width + padX * 2;
      ctx.fillStyle = rgba(style.boxColor, style.boxOpacity / 100);
      roundRect(ctx, cx - w / 2, y - padY, w, lineH + padY * 2, px * 0.22);
      ctx.fill();
    }
    if (style.shadow) {
      ctx.shadowColor = "rgba(0, 0, 0, 0.55)";
      ctx.shadowBlur = px * 0.22;
      ctx.shadowOffsetY = px * 0.06;
    }
    if (outline > 0) {
      ctx.strokeStyle = style.outlineColor;
      ctx.lineWidth = outline * 2;
      ctx.strokeText(line, cx, baseline);
      ctx.shadowColor = "transparent";
    }
    ctx.fillStyle = style.color;
    ctx.fillText(line, cx, baseline);
    ctx.shadowColor = "transparent";
  });
  ctx.restore();
}

/** One caption as a transparent PNG the size of the video frame. */
export async function captionPng(text: string, style: CaptionStyle, width: number, height: number): Promise<Uint8Array> {
  let blob: Blob;
  if (typeof OffscreenCanvas !== "undefined") {
    const canvas = new OffscreenCanvas(width, height);
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("Drawing isn't available.");
    drawCaption(ctx, text, style, width, height);
    blob = await canvas.convertToBlob({ type: "image/png" });
  } else {
    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("Drawing isn't available.");
    drawCaption(ctx, text, style, width, height);
    blob = await new Promise<Blob>((ok, fail) => canvas.toBlob((b) => (b ? ok(b) : fail(new Error("PNG encoding failed"))), "image/png"));
  }
  return new Uint8Array(await blob.arrayBuffer());
}
