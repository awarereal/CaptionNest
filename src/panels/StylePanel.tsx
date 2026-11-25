import { useEffect, useRef, type ReactNode } from "react";
import { Check, Plus, Trash, WarningCircle, X } from "@phosphor-icons/react";
import { toast } from "sonner";
import { useActiveTrack, useStore } from "../state/store";
import {
  CAPTION_FONTS,
  DEFAULT_STYLE,
  MARGIN_RANGE,
  PRESETS,
  SIZE_RANGE,
  WEIGHT_NAMES,
  allFonts,
  ensureFonts,
  fontById,
  fontMissing,
  matchingPreset,
  nearestWeight,
  type CaptionFont,
  type CaptionStyle,
} from "../lib/captionStyle";
import { drawCaption } from "../lib/renderCaption";
import { useUserFonts } from "../lib/userFonts";
import { pickOpenMany } from "../lib/fileFlows";
import { toAppError } from "../lib/api";
import { ask } from "../components/Confirm";
import { Button, IconButton, Switch } from "../components/ui";

const FONT_EXTENSIONS = ["ttf", "otf", "woff", "woff2"];

/** Asks for font files, adds them, and switches captions to the new font. */
async function addFonts(setStyle: (patch: Partial<CaptionStyle>) => void, weight: number) {
  const paths = await pickOpenMany({ filters: [{ name: "Fonts", extensions: FONT_EXTENSIONS }], title: "Add a caption font" });
  if (paths.length === 0) return;
  try {
    const result = await useUserFonts.getState().add(paths);
    for (const e of result.errors) toast.error(`${e.file} wasn't added`, { description: e.message });
    const id = result.added[result.added.length - 1];
    const font = id ? allFonts().find((f) => f.id === id) : undefined;
    if (font) {
      setStyle({ font: font.id, weight: nearestWeight(font, weight) });
      toast.success(`Added “${font.name}”`, {
        description: font.bangla === false ? "This font has no Bangla letters, so Bangla captions use Anek." : "Your captions now use it.",
      });
    }
  } catch (e) {
    toast.error("The font wasn't added", { description: toAppError(e).message });
  }
}

async function removeFont(font: CaptionFont, style: CaptionStyle, setStyle: (patch: Partial<CaptionStyle>) => void) {
  const choice = await ask({
    title: `Remove “${font.name}”?`,
    message: "Captions that use it switch to Anek. The file you added from stays where it is.",
    actions: [
      { id: "cancel", label: "Cancel", variant: "ghost" },
      { id: "remove", label: "Remove font", variant: "danger" },
    ],
  });
  if (choice !== "remove") return;
  try {
    await useUserFonts.getState().remove(font.id);
    if (style.font === font.id) setStyle({ font: DEFAULT_STYLE.font, weight: nearestWeight(CAPTION_FONTS[0], style.weight) });
  } catch (e) {
    toast.error("The font wasn't removed", { description: toAppError(e).message });
  }
}

const TEXT_COLORS = ["#ffffff", "#fff4dc", "#ffe14d", "#f2bd4b", "#7cc6bc", "#14110e"];
const LINE_COLORS = ["#000000", "#0b0907", "#ffffff", "#f2bd4b"];
const BOX_COLORS = ["#0b0907", "#000000", "#ffffff", "#f2bd4b", "#7cc6bc"];
const OUTLINES: { value: number; label: string }[] = [
  { value: 0, label: "None" },
  { value: 5, label: "Thin" },
  { value: 9, label: "Medium" },
  { value: 14, label: "Thick" },
];

/** A short line in the track's language for previews. */
function sampleFor(lang: string | undefined): string {
  if (lang === "bn") return "আজ আকাশ পরিষ্কার";
  if (lang === "hi") return "आज आसमान साफ़ है";
  if (lang === "ar") return "السماء صافية اليوم";
  return "Clear skies today";
}

export function StylePanel() {
  const style = useStore((s) => s.style);
  const setStyle = useStore((s) => s.setStyle);
  const close = () => useStore.getState().setStyleOpen(false);
  const track = useActiveTrack();
  const sample = sampleFor(track?.language);
  const active = matchingPreset(style);
  const font = fontById(style.font);
  // Re-render when added fonts change (the font list itself lives in captionStyle.ts).
  useUserFonts((s) => s.version);
  const fontsBusy = useUserFonts((s) => s.busy);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && close();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <aside className="stylepanel" aria-label="Caption style">
      <header className="stylepanel__head">
        <div>
          <h2 className="stylepanel__title">Caption style</h2>
          <p className="hint">
            Shown in the preview and burned into videos you export. SRT files carry only words and timing; CapCut styles those itself.
          </p>
        </div>
        <IconButton label="Close caption style" onClick={close}>
          <X size={18} />
        </IconButton>
      </header>

      <div className="stylepanel__body">
        <Section title="Presets">
          <div className="presets" role="radiogroup" aria-label="Presets">
            {PRESETS.map((p) => (
              <button
                key={p.id}
                role="radio"
                aria-checked={active === p.id}
                className={`preset ${active === p.id ? "is-on" : ""}`}
                onClick={() => setStyle(p.style)}
              >
                <StyleSwatch style={p.style} text={sample} />
                <span className="preset__name">{p.name}</span>
              </button>
            ))}
          </div>
        </Section>

        <Section title="Font">
          {fontMissing(style.font) && (
            <div className="fontmissing" role="status">
              <WarningCircle size={16} weight="fill" />
              <span>This project uses a font that was added on another computer. Captions show in Anek until you add that font here.</span>
            </div>
          )}
          <div className="fonts" role="radiogroup" aria-label="Font">
            {allFonts().map((f) => (
              <div key={f.id} className={`fontrow-wrap ${f.user ? "is-user" : ""}`}>
                <button
                  role="radio"
                  aria-checked={f.id === style.font}
                  className={`fontrow ${f.id === style.font ? "is-on" : ""}`}
                  onClick={() => setStyle({ font: f.id, weight: nearestWeight(f, style.weight) })}
                >
                  <span className="fontrow__sample" style={{ fontFamily: `${f.family}, "Anek Bangla Variable", sans-serif`, fontWeight: nearestWeight(f, 600) }}>
                    {f.name} <span lang="bn">· বাংলা</span>
                  </span>
                  <span className="fontrow__note">{f.user ? `Your font · ${f.note}` : f.note}</span>
                  {f.id === style.font && <Check className="fontrow__check" size={15} weight="bold" />}
                </button>
                {f.user && (
                  <IconButton label={`Remove ${f.name}`} size="sm" side="left" tone="danger" className="fontrow__remove" onClick={() => void removeFont(f, style, setStyle)}>
                    <Trash size={14} />
                  </IconButton>
                )}
              </div>
            ))}
            <button className="fontadd" onClick={() => void addFonts(setStyle, style.weight)} disabled={fontsBusy} aria-busy={fontsBusy || undefined}>
              <span className="fontadd__icon">{fontsBusy ? <span className="spinner" aria-hidden /> : <Plus size={16} weight="bold" />}</span>
              <span className="fontadd__text">
                <span className="fontadd__title">{fontsBusy ? "Adding font…" : "Add your own font"}</span>
                <span className="fontadd__note">.ttf, .otf, .woff or .woff2 · pick Regular and Bold together for both weights</span>
              </span>
            </button>
          </div>
          {font.weights.length > 1 && (
            <Segmented
              label="Weight"
              value={style.weight}
              options={font.weights.map((w) => ({ value: w, label: WEIGHT_NAMES[w] ?? String(w) }))}
              onChange={(weight) => setStyle({ weight })}
            />
          )}
          <Slider
            label="Size"
            value={style.size}
            min={SIZE_RANGE[0]}
            max={SIZE_RANGE[1]}
            step={0.1}
            display={style.size.toFixed(1)}
            onChange={(size) => setStyle({ size })}
          />
          <Toggle id="st-upper" label="Capital letters" hint="Latin letters only" checked={style.uppercase} onChange={(uppercase) => setStyle({ uppercase })} />
        </Section>

        <Section title="Color">
          <Swatches label="Text" colors={TEXT_COLORS} value={style.color} onChange={(color) => setStyle({ color })} />
          <Segmented
            label="Outline"
            value={OUTLINES.reduce((b, o) => (Math.abs(o.value - style.outline) < Math.abs(b.value - style.outline) ? o : b)).value}
            options={OUTLINES}
            onChange={(outline) => setStyle({ outline })}
          />
          {style.outline > 0 && <Swatches label="Outline color" colors={LINE_COLORS} value={style.outlineColor} onChange={(outlineColor) => setStyle({ outlineColor })} />}
          <Toggle id="st-shadow" label="Soft shadow" checked={style.shadow} onChange={(shadow) => setStyle({ shadow })} />
          <Toggle id="st-box" label="Background" hint="A band behind each line" checked={style.box} onChange={(box) => setStyle({ box })} />
          {style.box && (
            <>
              <Swatches label="Background color" colors={BOX_COLORS} value={style.boxColor} onChange={(boxColor) => setStyle({ boxColor })} />
              <Slider
                label="Background opacity"
                value={style.boxOpacity}
                min={10}
                max={100}
                step={1}
                display={`${Math.round(style.boxOpacity)}%`}
                onChange={(boxOpacity) => setStyle({ boxOpacity })}
              />
            </>
          )}
        </Section>

        <Section title="Position">
          <Segmented
            label="Place"
            value={style.position}
            options={[
              { value: "top", label: "Top" },
              { value: "middle", label: "Middle" },
              { value: "bottom", label: "Bottom" },
            ]}
            onChange={(position) => setStyle({ position })}
          />
          {style.position !== "middle" && (
            <Slider
              label={style.position === "top" ? "Distance from top" : "Distance from bottom"}
              value={style.margin}
              min={MARGIN_RANGE[0]}
              max={MARGIN_RANGE[1]}
              step={1}
              display={`${Math.round(style.margin)}%`}
              onChange={(margin) => setStyle({ margin })}
            />
          )}
        </Section>
      </div>

      <footer className="stylepanel__foot">
        <span className="hint">New projects start with this style.</span>
        <Button variant="primary" onClick={close}>
          Done
        </Button>
      </footer>
    </aside>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="stsec">
      <h3 className="stsec__title">{title}</h3>
      <div className="stsec__body">{children}</div>
    </section>
  );
}

function Segmented<T extends string | number>({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
}) {
  return (
    <div className="stfield">
      <span className="stfield__label">{label}</span>
      <div className="seg" role="radiogroup" aria-label={label}>
        {options.map((o) => (
          <button key={String(o.value)} role="radio" aria-checked={o.value === value} className={`seg__opt ${o.value === value ? "is-on" : ""}`} onClick={() => onChange(o.value)}>
            {o.label}
          </button>
        ))}
      </div>
    </div>
  );
}

function Slider({
  label,
  value,
  min,
  max,
  step,
  display,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  display: string;
  onChange: (v: number) => void;
}) {
  const pct = ((value - min) / (max - min)) * 100;
  return (
    <label className="stfield">
      <span className="stfield__label">
        {label}
        <span className="stfield__value mono">{display}</span>
      </span>
      <input
        type="range"
        className="strange"
        style={{ ["--pct" as string]: `${pct}%` }}
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
      />
    </label>
  );
}

function Toggle({ id, label, hint, checked, onChange }: { id: string; label: string; hint?: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <div className="sttoggle">
      <label htmlFor={id} className="sttoggle__text">
        <span>{label}</span>
        {hint && <span className="hint">{hint}</span>}
      </label>
      <Switch id={id} label={label} checked={checked} onChange={onChange} />
    </div>
  );
}

function Swatches({ label, colors, value, onChange }: { label: string; colors: string[]; value: string; onChange: (c: string) => void }) {
  const custom = !colors.includes(value);
  return (
    <div className="stfield">
      <span className="stfield__label">{label}</span>
      <div className="swatches" role="radiogroup" aria-label={label}>
        {colors.map((c) => (
          <button
            key={c}
            role="radio"
            aria-checked={c === value}
            aria-label={c}
            className={`swatch ${c === value ? "is-on" : ""}`}
            style={{ ["--c" as string]: c }}
            onClick={() => onChange(c)}
          />
        ))}
        <label className={`swatch swatch--custom ${custom ? "is-on" : ""}`} style={custom ? { ["--c" as string]: value } : undefined} title="Choose any color">
          {!custom && <Plus size={13} weight="bold" />}
          <input type="color" value={value} onChange={(e) => onChange(e.target.value)} aria-label={`${label}: any color`} />
        </label>
      </div>
    </div>
  );
}

/** A small frame showing a style, drawn by the same renderer as the video. */
function StyleSwatch({ style, text }: { style: CaptionStyle; text: string }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const fontsVersion = useUserFonts((s) => s.version);
  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    let alive = true;
    const dpr = window.devicePixelRatio || 1;
    const w = Math.round(132 * dpr);
    const h = Math.round(74 * dpr);
    const draw = () => {
      if (!alive) return;
      canvas.width = w;
      canvas.height = h;
      const ctx = canvas.getContext("2d");
      if (!ctx) return;
      // Text is drawn larger than true scale so the look reads at thumbnail size.
      drawCaption(ctx, text, { ...style, size: style.size * 2.7, margin: Math.min(style.margin, 12) }, w, h);
    };
    draw();
    void ensureFonts(style, text).then(draw);
    return () => {
      alive = false;
    };
  }, [style, text, fontsVersion]);
  return <canvas ref={ref} className="preset__frame" aria-hidden />;
}
