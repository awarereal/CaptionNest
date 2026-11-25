// Promise-based confirmation dialogs: `const choice = await ask({...})`.
import { create } from "zustand";
import { AlertDialog } from "radix-ui";
import type { ReactNode } from "react";

export interface AskAction {
  id: string;
  label: string;
  variant?: "primary" | "secondary" | "ghost" | "danger";
}

interface AskOptions {
  title: string;
  message?: ReactNode;
  actions: AskAction[];
  /** Returned when the dialog is dismissed with Escape. */
  cancelId?: string;
}

interface AskState {
  current: (AskOptions & { resolve: (id: string) => void }) | null;
}

const useAsk = create<AskState>(() => ({ current: null }));

export function ask(options: AskOptions): Promise<string> {
  return new Promise((resolve) => {
    useAsk.setState({ current: { ...options, resolve } });
  });
}

export function ConfirmHost() {
  const current = useAsk((s) => s.current);
  const close = (id: string) => {
    current?.resolve(id);
    useAsk.setState({ current: null });
  };
  return (
    <AlertDialog.Root open={!!current} onOpenChange={(open) => !open && close(current?.cancelId ?? "cancel")}>
      <AlertDialog.Portal>
        <AlertDialog.Overlay className="overlay" />
        <AlertDialog.Content className="modal modal--sm confirm">
          <div className="modal__head">
            <AlertDialog.Title className="modal__title display">{current?.title}</AlertDialog.Title>
          </div>
          <div className="modal__body">
            <AlertDialog.Description asChild>
              <div className="confirm__message">{current?.message}</div>
            </AlertDialog.Description>
          </div>
          <div className="modal__foot">
            {current?.actions.map((a, i) => (
              <button
                key={a.id}
                className={`btn btn--md btn--${a.variant ?? "secondary"}`}
                onClick={() => close(a.id)}
                autoFocus={i === current.actions.length - 1}
              >
                <span className="btn__label">{a.label}</span>
              </button>
            ))}
          </div>
        </AlertDialog.Content>
      </AlertDialog.Portal>
    </AlertDialog.Root>
  );
}
