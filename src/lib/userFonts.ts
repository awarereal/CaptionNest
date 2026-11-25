// Caption fonts the user adds. The files live in the app's data folder; each one is read
// as bytes and registered as a FontFace (the content policy allows no font URLs), under a
// private family name so it can't clash with fonts installed on the computer. Previews
// and burned-in exports then use it like any built-in caption font.
import { create } from "zustand";
import { api, toAppError } from "./api";
import { setUserFonts, type CaptionFont } from "./captionStyle";
import type { AppError, FontImportResult, UserFont } from "./types";

/** Weights offered for a variable font, within its range. */
const STEPS = [100, 200, 300, 400, 500, 600, 700, 800, 900];

export const userFamily = (id: string) => `"CN User ${id}"`;

/** How an added font appears in the caption style panel. */
export function toCaptionFont(f: UserFont): CaptionFont {
  const upright = f.faces.filter((x) => !x.italic);
  const faces = upright.length > 0 ? upright : f.faces;
  const weights = new Set<number>();
  for (const face of faces) {
    if (face.weightMax) STEPS.filter((w) => w >= face.weight && w <= face.weightMax!).forEach((w) => weights.add(w));
    else weights.add(face.weight);
  }
  const list = [...weights].sort((a, b) => a - b);
  const bangla = faces.some((x) => x.bangla === true) ? true : faces.every((x) => x.bangla === false) ? false : null;
  const variable = faces.some((x) => x.weightMax);
  const styles = variable ? "Variable weight" : list.length === 1 ? "1 style" : `${list.length} weights`;
  return {
    id: f.id,
    name: f.name,
    family: userFamily(f.id),
    weights: list.length > 0 ? list : [400],
    note: bangla === false ? `${styles} · Latin letters only` : styles,
    user: true,
    bangla,
  };
}

const loaded = new Map<string, FontFace[]>();
/** The files each registered font was loaded from, to notice added or replaced files. */
const loadedFiles = new Map<string, string>();

async function register(font: UserFont): Promise<void> {
  const files = font.faces.map((f) => f.file).join("|");
  if (loaded.has(font.id) && loadedFiles.get(font.id) === files) return;
  unregister(font.id);
  const faces: FontFace[] = [];
  // Tracked before loading, so a face that fails later can still be removed.
  loaded.set(font.id, faces);
  const upright = font.faces.some((x) => !x.italic);
  for (const face of font.faces) {
    const bytes = await api.fontFile(face.file);
    const ff = new FontFace(userFamily(font.id).slice(1, -1), bytes, {
      weight: face.weightMax ? `${face.weight} ${face.weightMax}` : String(face.weight),
      // With only italic files, use them as the normal style rather than not at all.
      style: face.italic && upright ? "italic" : "normal",
    });
    await ff.load();
    document.fonts.add(ff);
    faces.push(ff);
  }
  loadedFiles.set(font.id, files);
}

function unregister(id: string) {
  for (const ff of loaded.get(id) ?? []) document.fonts.delete(ff);
  loaded.delete(id);
  loadedFiles.delete(id);
}

interface FontsState {
  fonts: UserFont[];
  /** Bumped whenever font files finish loading, so canvases redraw. */
  version: number;
  busy: boolean;
  load: () => Promise<void>;
  add: (paths: string[]) => Promise<FontImportResult>;
  remove: (id: string) => Promise<void>;
}

export const useUserFonts = create<FontsState>((set, get) => {
  /** Registers every font in `list`, drops fonts that are gone, and publishes the rest. */
  const apply = async (list: UserFont[]): Promise<{ id: string; error: AppError }[]> => {
    for (const id of [...loaded.keys()]) if (!list.some((f) => f.id === id)) unregister(id);
    const failed: { id: string; error: AppError }[] = [];
    const ok: UserFont[] = [];
    for (const font of list) {
      try {
        await register(font);
        ok.push(font);
      } catch (e) {
        unregister(font.id);
        failed.push({ id: font.id, error: toAppError(e) });
      }
    }
    setUserFonts(ok.map(toCaptionFont));
    set({ fonts: ok, version: get().version + 1 });
    return failed;
  };

  return {
    fonts: [],
    version: 0,
    busy: false,
    async load() {
      try {
        await apply(await api.listFonts());
      } catch {
        // Added fonts are optional; captions fall back to the built-in ones.
      }
    },
    async add(paths) {
      set({ busy: true });
      try {
        const result = await api.importFonts(paths);
        const failed = await apply(result.fonts);
        for (const f of failed) {
          const name = result.fonts.find((x) => x.id === f.id)?.name ?? "A font";
          result.errors.push({ file: name, message: "This font couldn't be loaded. The file may be damaged." });
          await api.removeFont(f.id).catch(() => undefined);
        }
        result.added = result.added.filter((id) => !failed.some((f) => f.id === id));
        return result;
      } finally {
        set({ busy: false });
      }
    },
    async remove(id) {
      const list = await api.removeFont(id);
      await apply(list);
    },
  };
});
