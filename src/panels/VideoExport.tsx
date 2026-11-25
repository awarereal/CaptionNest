import { useEffect, useRef } from "react";
import { FilmStrip, TextAa } from "@phosphor-icons/react";
import { ensureFonts, fontById, type CaptionStyle } from "../lib/captionStyle";
import { drawCaption } from "../lib/renderCaption";
import { sortCues } from "../lib/cueOps";
import { useStore } from "../state/store";
import { Button } from "../components/ui";
import { useExport } from "./ExportDialog";
import type { MediaInfo, Track } from "../lib/types";

/** Picture size of the exported video: the source frame as players show it (rotated). */
export function videoFrameSize(media: MediaInfo | null): { width: number; height: number } | null {
  const v = media?.video;
  if (!v || v.width <= 0 || v.height <= 0) return null;
  const turned = v.rotation % 180 === 90;
  const width = turned ? v.height : v.width;
  const height = turned ? v.width : v.height;
  // H.264 needs even sizes; FFmpeg rounds the same way.
  return { width: width - (width % 2), height: height - (height % 2) };
}

/** What the export will look like: the first caption drawn at the video's shape. */
export function VideoExport({ track, style, frame }: { track: Track | null; style: CaptionStyle; frame: { width: number; height: number } | null }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const first = track ? sortCues(track.cues).find((c) => c.text.trim()) : undefined;
  const text = first?.text ?? "";
  const aspect = frame ? frame.width / frame.height : 16 / 9;

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    let alive = true;
    const dpr = window.devicePixelRatio || 1;
    const cssH = 150;
    const w = Math.round(cssH * aspect * dpr);
    const h = Math.round(cssH * dpr);
    const draw = () => {
      if (!alive) return;
      canvas.width = w;
      canvas.height = h;
      const ctx = canvas.getContext("2d");
      if (ctx && text) drawCaption(ctx, text, style, w, h);
    };
    draw();
    void ensureFonts(style, text).then(draw);
    return () => {
      alive = false;
    };
  }, [style, text, aspect]);

  const openStyle = () => {
    useExport.getState().close();
    useStore.getState().setStyleOpen(true);
  };

  return (
    <>
      <div className="vexp">
        <canvas ref={ref} className="vexp__frame" style={{ aspectRatio: `${aspect}` }} aria-hidden />
        <div className="vexp__style">
          <p className="vexp__font">
            <TextAa size={16} /> {fontById(style.font).name}
          </p>
          <p className="hint">
            {style.position === "top" ? "Top" : style.position === "middle" ? "Middle" : "Bottom"} · size {style.size.toFixed(1)}
            {style.box ? " · background" : style.outline > 0 ? " · outline" : ""}
          </p>
          <Button size="sm" variant="secondary" onClick={openStyle}>
            Change style
          </Button>
        </div>
      </div>
      <div className="export-file">
        <FilmStrip size={20} />
        <div>
          <p className="export-file__name">MP4 video{frame ? ` · ${frame.width}×${frame.height}` : ""}</p>
          <p className="hint">H.264 with the original sound · {track ? sortCues(track.cues).filter((c) => c.text.trim()).length : 0} captions drawn in</p>
        </div>
      </div>
    </>
  );
}
