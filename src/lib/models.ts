// How speech models are presented. Sizes, hashes and URLs come from the Rust catalog.
//
// Recommendations come from CaptionNest's benchmark on real Bangla speech (FLEURS):
//   Apple Silicon (GPU): Accurate had the lowest error rate, at about real time.
//   CPU only (Windows / Intel Mac): Accurate ran 4.4x slower than real time, while Fast
//   (with Bengali script correction) matched its accuracy at about real time.

import type { AppInfo } from "./types";

export interface ModelInfo {
  id: string;
  name: string;
  summary: string;
  speed: 1 | 2 | 3;
  accuracy: 1 | 2 | 3;
  translates: boolean;
  bestFor: string;
}

export const MODEL_INFO: Record<string, ModelInfo> = {
  "large-v3-q5_0": {
    id: "large-v3-q5_0",
    name: "Accurate",
    summary:
      "Highest accuracy, and the only model that can turn speech in any language into English captions. Best on Apple Silicon Macs; slow on computers without a supported graphics chip.",
    speed: 1,
    accuracy: 3,
    translates: true,
    bestFor: "Best accuracy · English captions from other languages",
  },
  "large-v3-turbo-q5_0": {
    id: "large-v3-turbo-q5_0",
    name: "Fast",
    summary:
      "Nearly as accurate for most languages, Bangla included, and about four times faster. Captions stay in the spoken language or use offline text translation.",
    speed: 3,
    accuracy: 2,
    translates: false,
    bestFor: "Quick captions in the spoken language",
  },
  "large-v3-turbo-bn-q5_0": {
    id: "large-v3-turbo-bn-q5_0",
    name: "Bangla",
    summary:
      "Trained by Mozilla AI on Bangla speech, so it spells Bangla far more accurately than the general models. Used automatically whenever the speech is Bangla.",
    speed: 3,
    accuracy: 3,
    translates: false,
    bestFor: "Bangla speech with correct spelling",
  },
  "large-v3-bn-q5_0": {
    id: "large-v3-bn-q5_0",
    name: "Bangla Max",
    summary:
      "The larger Bangla-trained model: slightly fewer spelling mistakes than Bangla, but about four times slower. Used instead of Bangla when both are installed.",
    speed: 1,
    accuracy: 3,
    translates: false,
    bestFor: "Bangla speech, highest accuracy",
  },
  small: {
    id: "small",
    name: "Light",
    summary: "The smallest model, for older computers. Fine for clear English; it can't write Bangla in Bengali script.",
    speed: 3,
    accuracy: 1,
    translates: true,
    bestFor: "Clear English speech on older computers",
  },
};

export const DEFAULT_MODEL_ID = "large-v3-q5_0";

/** The model to suggest first on this computer. */
export function recommendedModelId(info: AppInfo | null): string {
  if (info?.os === "macos" && info.arch === "aarch64") return "large-v3-q5_0";
  return "large-v3-turbo-q5_0";
}

/** The installed-or-not model trained for a language, if the catalog has one. */
export const languageModelFor = <T extends { language?: string | null }>(models: T[] | undefined, lang: string) =>
  (models ?? []).find((m) => m.language === lang);

export const modelName = (id: string | null | undefined) => (id && MODEL_INFO[id]?.name) || id || "No model";
