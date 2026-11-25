import { useEffect, useState } from "react";
import { Tooltip } from "radix-ui";
import { Toaster } from "sonner";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { api, events } from "./lib/api";
import { useStore } from "./state/store";
import { useVoice } from "./state/voice";
import { useUserFonts } from "./lib/userFonts";
import { enableAutomation } from "./lib/automation";
import { applyTheme } from "./lib/theme";
import { confirmDiscard, openPath } from "./lib/fileFlows";
import { ConfirmHost } from "./components/Confirm";
import { TopBar } from "./components/TopBar";
import { HomeScreen } from "./screens/HomeScreen";
import { SetupScreen } from "./screens/SetupScreen";
import { PrepareScreen } from "./screens/PrepareScreen";
import { WorkspaceScreen } from "./screens/WorkspaceScreen";
import { SettingsPanel } from "./panels/SettingsPanel";
import { HelpPanel } from "./panels/HelpPanel";
import { AboutPanel } from "./panels/AboutPanel";
import { DropVeil } from "./components/DropVeil";

export default function App() {
  const view = useStore((s) => s.view);
  const [dragging, setDragging] = useState(false);
  const [bootError, setBootError] = useState<string | null>(null);

  // Boot: load state, subscribe to backend events, then reveal the window (no blank flash).
  useEffect(() => {
    const st = useStore.getState();
    // The theme follows the settings as soon as they change (and before the window
    // first shows, so a light theme never flashes dark).
    const unsubTheme = useStore.subscribe((s, prev) => {
      if (s.settings?.theme !== prev.settings?.theme) applyTheme(s.settings?.theme);
    });
    const unlisten = [
      events.onJobProgress(st.onJobProgress),
      events.onJobFinished(st.onJobFinished),
      events.onDownload(st.onDownload),
    ];
    st.init()
      .then(() => {
        if (useStore.getState().info?.automation) enableAutomation(useStore);
      })
      .catch((e) => setBootError(String(e?.message ?? e)))
      .finally(() => requestAnimationFrame(() => void api.showWindow()));
    // Caption fonts the user added load in the background; previews redraw when ready.
    void useUserFonts.getState().load();
    void useVoice.getState().refreshStatus();
    return () => {
      unsubTheme();
      unlisten.forEach((p) => p.then((fn) => fn()));
    };
  }, []);

  // Files dropped anywhere on the window.
  useEffect(() => {
    const p = getCurrentWebview().onDragDropEvent((event) => {
      const t = event.payload.type;
      if (t === "enter" || t === "over") setDragging(true);
      else if (t === "leave") setDragging(false);
      else if (t === "drop") {
        setDragging(false);
        const path = event.payload.paths[0];
        if (path && useStore.getState().view !== "setup") void openPath(path);
      }
    });
    return () => void p.then((fn) => fn());
  }, []);

  // Closing the window with unsaved edits asks first.
  useEffect(() => {
    const win = getCurrentWindow();
    const p = win.onCloseRequested(async (event) => {
      if (!useStore.getState().dirty) return;
      event.preventDefault();
      if (await confirmDiscard("Your caption edits haven't been saved. Save them before closing?")) {
        await win.destroy();
      }
    });
    return () => void p.then((fn) => fn());
  }, []);

  if (bootError) {
    return (
      <div className="boot-error">
        <h1 className="display">CaptionNest couldn't start</h1>
        <p>{bootError}</p>
        <p className="hint">Reinstalling the app usually fixes this.</p>
      </div>
    );
  }

  return (
    <Tooltip.Provider delayDuration={350} skipDelayDuration={150}>
      <div className="app" data-view={view}>
        <TopBar />
        <main className="app__main">
          {/* Screens swap immediately (keyed for a CSS entrance), so the interface never
              waits on animation frames, which browsers pause for hidden windows. */}
          <div key={view} className="screen screen--enter">
            {view === "setup" && <SetupScreen />}
            {view === "home" && <HomeScreen />}
            {view === "prepare" && <PrepareScreen />}
            {view === "workspace" && <WorkspaceScreen />}
            {view === "loading" && <div className="loading-screen" aria-busy />}
          </div>
        </main>
        <DropVeil visible={dragging && view !== "setup" && view !== "loading"} />
      </div>
      <SettingsPanel />
      <HelpPanel />
      <AboutPanel />
      <ConfirmHost />
      <Toaster position="bottom-right" toastOptions={{ unstyled: true, className: "toast" }} />
    </Tooltip.Provider>
  );
}
