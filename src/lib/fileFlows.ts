// File dialogs and the flows around them (open, save, unsaved-changes guard).
import { open, save } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import { ask } from "../components/Confirm";
import { useStore } from "../state/store";
import { toAppError } from "./api";
import { describeError } from "./errors";

export const VIDEO_EXTENSIONS = [
  "mp4", "mov", "m4v", "mkv", "webm", "avi", "mts", "m2ts", "ts", "3gp", "flv", "wmv", "mpg", "mpeg",
  "mp3", "m4a", "wav", "aac", "flac", "ogg", "opus",
];
export const PROJECT_EXTENSION = "captionnest";

/**
 * Test automation only: scripts may queue paths that stand in for the native file
 * dialogs, which web automation can't click. Unused in normal operation.
 */
function automationPath(): string | null | undefined {
  const q = (window as unknown as { __CN_AUTOMATION__?: { paths: string[] } }).__CN_AUTOMATION__;
  return q && q.paths.length > 0 ? q.paths.shift() : undefined;
}

export async function pickOpen(options: Parameters<typeof open>[0]): Promise<string | null> {
  const queued = automationPath();
  if (queued !== undefined) return queued;
  const r = await open(options);
  return typeof r === "string" ? r : null;
}

/** Like pickOpen, but the user may choose several files. */
export async function pickOpenMany(options: Omit<NonNullable<Parameters<typeof open>[0]>, "multiple">): Promise<string[]> {
  const queued = automationPath();
  if (queued !== undefined) return queued ? [queued] : [];
  const r = await open({ ...options, multiple: true });
  return Array.isArray(r) ? r : typeof r === "string" ? [r] : [];
}

export async function pickSave(options: Parameters<typeof save>[0]): Promise<string | null> {
  const queued = automationPath();
  if (queued !== undefined) return queued;
  return (await save(options)) ?? null;
}

export const isProjectPath = (p: string) => p.toLowerCase().endsWith(`.${PROJECT_EXTENSION}`);
export const isMediaPath = (p: string) => VIDEO_EXTENSIONS.includes(p.split(".").pop()?.toLowerCase() ?? "");

/** Asks what to do with unsaved edits. Returns false if the user cancels. */
export async function confirmDiscard(reason = "Your caption edits haven't been saved."): Promise<boolean> {
  if (!useStore.getState().dirty) return true;
  const choice = await ask({
    title: "Save changes first?",
    message: reason,
    actions: [
      { id: "cancel", label: "Cancel", variant: "ghost" },
      { id: "discard", label: "Don't save", variant: "secondary" },
      { id: "save", label: "Save", variant: "primary" },
    ],
  });
  if (choice === "cancel") return false;
  if (choice === "save") return await saveProject();
  return true;
}

export async function browseVideo() {
  const picked = await pickOpen({
    multiple: false,
    directory: false,
    title: "Choose a video",
    filters: [{ name: "Video and audio", extensions: VIDEO_EXTENSIONS }],
  });
  if (typeof picked === "string") await openPath(picked);
}

export async function browseProject() {
  const picked = await pickOpen({
    multiple: false,
    title: "Open a CaptionNest project",
    filters: [{ name: "CaptionNest project", extensions: [PROJECT_EXTENSION] }],
  });
  if (typeof picked === "string") await openPath(picked);
}

/** Opens a dropped or chosen file: projects open in the editor, media starts a new job. */
export async function openPath(path: string) {
  const st = useStore.getState();
  if (st.job) {
    toast("Captions are still being made", { description: "Stop the current job before opening another file." });
    return;
  }
  if (!(await confirmDiscard())) return;
  try {
    if (isProjectPath(path)) await useStore.getState().loadProject(path);
    else if (isMediaPath(path)) await useStore.getState().openVideo(path);
    else {
      toast.error("This file type isn't supported", {
        description: "Open a video (MP4, MOV, MKV, WebM…) or a .captionnest project.",
        className: "toast toast--error",
      });
    }
  } catch (e) {
    const d = describeError(toAppError(e));
    toast.error(d.title, { description: d.message, className: "toast toast--error" });
  }
}

/** Saves to the current project file, or asks where on first save. */
export async function saveProject(saveAs = false): Promise<boolean> {
  const { project, projectPath, saveProject: doSave } = useStore.getState();
  if (!project) return false;
  let path = projectPath;
  if (!path || saveAs) {
    const picked = await pickSave({
      title: "Save project",
      defaultPath: `${project.video.fileName.replace(/\.[^.]+$/, "")}.${PROJECT_EXTENSION}`,
      filters: [{ name: "CaptionNest project", extensions: [PROJECT_EXTENSION] }],
    });
    if (!picked) return false;
    path = picked.toLowerCase().endsWith(`.${PROJECT_EXTENSION}`) ? picked : `${picked}.${PROJECT_EXTENSION}`;
  }
  try {
    await doSave(path);
    toast("Project saved", { description: path.split(/[\\/]/).pop() });
    return true;
  } catch (e) {
    const d = describeError(toAppError(e));
    toast.error("Couldn't save the project", { description: d.message, className: "toast toast--error" });
    return false;
  }
}
