import { ArrowLeft, FloppyDisk, GearSix, Question, Export } from "@phosphor-icons/react";
import { useStore } from "../state/store";
import { Wordmark } from "./Logo";
import { Button, IconButton } from "./ui";
import { confirmDiscard, saveProject } from "../lib/fileFlows";
import { useExport } from "../panels/ExportDialog";

const isMac = /Mac/i.test(navigator.userAgent);

export function TopBar() {
  const view = useStore((s) => s.view);
  const project = useStore((s) => s.project);
  const media = useStore((s) => s.media);
  const dirty = useStore((s) => s.dirty);
  const projectPath = useStore((s) => s.projectPath);
  const job = useStore((s) => s.job);
  const openPanel = useStore((s) => s.openPanel);
  const startExport = useExport((s) => s.start);

  const inWork = view === "workspace" && project;
  const title = inWork
    ? (projectPath?.split(/[\\/]/).pop()?.replace(/\.captionnest$/i, "") ?? project.video.fileName)
    : view === "prepare"
      ? media?.fileName
      : null;

  const goHome = async () => {
    if (job) return;
    if (!(await confirmDiscard())) return;
    useStore.getState().closeProject();
  };

  return (
    <header className={`topbar ${isMac ? "topbar--mac" : ""}`} data-tauri-drag-region>
      <div className="topbar__left" data-tauri-drag-region>
        {view === "workspace" || view === "prepare" ? (
          <>
            <IconButton label="Back to start" onClick={goHome} disabled={!!job}>
              <ArrowLeft size={18} />
            </IconButton>
            <div className="topbar__title" data-tauri-drag-region>
              <span className="topbar__name selectable" title={title ?? undefined}>
                {title}
              </span>
              {inWork && (
                <span className={`topbar__state ${dirty ? "is-dirty" : ""}`}>{dirty ? "Unsaved changes" : "Saved"}</span>
              )}
            </div>
          </>
        ) : null}
      </div>
      {view !== "workspace" && view !== "prepare" && (
        <div className="topbar__center" data-tauri-drag-region>
          <Wordmark />
        </div>
      )}
      <div className="topbar__right">
        {inWork && (
          <>
            <Button variant="ghost" size="sm" icon={<FloppyDisk size={17} />} onClick={() => void saveProject()} title="Save project">
              Save
            </Button>
            <Button variant="primary" size="sm" icon={<Export size={17} weight="bold" />} onClick={startExport} disabled={!!job}>
              Export SRT for CapCut
            </Button>
            <span className="topbar__sep" />
          </>
        )}
        <IconButton label="How to use captions in CapCut" onClick={() => openPanel("help")}>
          <Question size={19} />
        </IconButton>
        <IconButton label="Settings" shortcut="Mod+," onClick={() => openPanel("settings")}>
          <GearSix size={19} />
        </IconButton>
      </div>
    </header>
  );
}
