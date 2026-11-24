import { AnimatePresence, motion } from "motion/react";
import { useStore } from "../state/store";

/** Full-window target shown while a file is dragged over the app. */
export function DropVeil({ visible }: { visible: boolean }) {
  const view = useStore((s) => s.view);
  const replacing = view === "workspace" || view === "prepare";
  return (
    <AnimatePresence>
      {visible && (
        <motion.div
          className="dropveil"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.16 }}
          aria-hidden
        >
          <div className="dropveil__frame">
            <span className="crop crop--tl" />
            <span className="crop crop--tr" />
            <span className="crop crop--bl" />
            <span className="crop crop--br" />
            <p className="dropveil__title display">Release to open</p>
            <p className="dropveil__sub">{replacing ? "This replaces what's open now." : "Videos and .captionnest projects"}</p>
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
