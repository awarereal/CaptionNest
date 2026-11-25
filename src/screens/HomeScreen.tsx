import { useEffect, useState } from "react";
import { FilmSlate, FolderOpen, X, Warning, CheckCircle, ClockCounterClockwise } from "@phosphor-icons/react";
import { useStore } from "../state/store";
import { browseProject, browseVideo, openPath } from "../lib/fileFlows";
import { api } from "../lib/api";
import { Button, IconButton } from "../components/ui";
import { modelName } from "../lib/models";

function relativeDate(ms: number) {
  const diff = Date.now() - ms;
  const day = 86_400_000;
  if (diff < 60_000) return "just now";
  if (diff < 3_600_000) return `${Math.round(diff / 60_000)} min ago`;
  if (diff < day) return `${Math.round(diff / 3_600_000)} h ago`;
  if (diff < 7 * day) return `${Math.round(diff / day)} d ago`;
  return new Date(ms).toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" });
}

export function HomeScreen() {
  const settings = useStore((s) => s.settings);
  const models = useStore((s) => s.models);
  const opening = useStore((s) => s.opening);
  const openPanel = useStore((s) => s.openPanel);
  const setView = useStore((s) => s.setView);
  const [missing, setMissing] = useState<Set<string>>(new Set());

  const recent = settings?.recentProjects ?? [];
  const activeModel = models?.whisper.find((m) => m.id === settings?.transcribeModelId && m.installed);

  useEffect(() => {
    let alive = true;
    Promise.all(recent.map(async (r) => [r.path, await api.pathExists(r.path)] as const)).then((rows) => {
      if (alive) setMissing(new Set(rows.filter(([, ok]) => !ok).map(([p]) => p)));
    });
    return () => {
      alive = false;
    };
  }, [recent]);

  const forget = async (path: string) => {
    await api.forgetRecent(path);
    const s = await api.getSettings();
    useStore.setState((st) => (st.settings ? { settings: { ...st.settings, recentProjects: s.recentProjects } } : {}));
  };

  return (
    <div className="home">
      <section className="gate" aria-labelledby="gate-title">
        <span className="crop crop--tl" aria-hidden />
        <span className="crop crop--tr" aria-hidden />
        <span className="crop crop--bl" aria-hidden />
        <span className="crop crop--br" aria-hidden />

        <div className="gate__sample" aria-hidden>
          <span className="sample-cue caption-text" lang="bn">
            আজ আমরা নদীর ধারে বেড়াতে যাব।
          </span>
          <span className="sample-cue sample-cue--en caption-text">Today we're going for a walk by the river.</span>
        </div>

        <h1 id="gate-title" className="gate__title display">
          Drop a video to caption it
        </h1>
        <p className="gate__lede">
          CaptionNest listens to the speech, writes the captions and gets them ready for CapCut. It all happens on this
          computer; nothing is uploaded.
        </p>
        <div className="gate__actions">
          <Button variant="primary" size="lg" icon={<FilmSlate size={20} weight="bold" />} onClick={() => void browseVideo()} busy={opening}>
            Choose video
          </Button>
          <Button variant="secondary" size="lg" icon={<FolderOpen size={20} />} onClick={() => void browseProject()}>
            Open project
          </Button>
        </div>
        <p className="gate__formats">MP4 · MOV · MKV · WebM · and most audio files</p>
      </section>

      {recent.length > 0 && (
        <section className="recent" aria-labelledby="recent-title">
          <h2 id="recent-title" className="section-label">
            <ClockCounterClockwise size={15} /> Recent projects
          </h2>
          <ul className="recent__list">
            {recent.slice(0, 6).map((r) => {
              const gone = missing.has(r.path);
              return (
                <li key={r.path} className={`recent__item ${gone ? "is-missing" : ""}`}>
                  <button className="recent__open" onClick={() => void openPath(r.path)} disabled={gone}>
                    <span className="recent__name">{r.name}</span>
                    <span className="recent__meta">
                      {gone ? "File moved or deleted" : r.videoName} · {relativeDate(r.openedAtMs)}
                    </span>
                  </button>
                  <IconButton label="Remove from list" size="sm" onClick={() => void forget(r.path)}>
                    <X size={14} />
                  </IconButton>
                </li>
              );
            })}
          </ul>
        </section>
      )}

      <footer className="home__status">
        {activeModel ? (
          <button className="status-chip" onClick={() => openPanel("settings", "models")}>
            <CheckCircle size={16} weight="fill" className="status-chip__ok" />
            <span>
              Speech model <strong>{modelName(activeModel.id)}</strong>, ready offline
            </span>
          </button>
        ) : (
          <button className="status-chip status-chip--warn" onClick={() => setView("setup")}>
            <Warning size={16} weight="fill" />
            No speech model yet — <strong>set up now</strong>
          </button>
        )}
        <p className="madeby">
          Made by <span className="madeby__name">Aware</span>
        </p>
      </footer>
    </div>
  );
}
