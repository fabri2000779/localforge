/** Promise-based in-app dialogs (confirm / alert / prompt) rendered by <AppDialog>; replaces the native window.* popups. */
import { create } from 'zustand';

export interface DialogRequest {
  kind: 'confirm' | 'alert' | 'prompt';
  title: string;
  message?: string;
  confirmLabel?: string;
  cancelLabel?: string;
  /** Red primary button for destructive actions. */
  danger?: boolean;
  /** Prompt: field label, initial text and whether an empty answer may be submitted. */
  label?: string;
  defaultValue?: string;
  placeholder?: string;
  allowEmpty?: boolean;
}

type Answer = boolean | string | null;

export interface ActiveDialog extends DialogRequest {
  id: number;
  resolve: (value: Answer) => void;
}

interface DialogState {
  current: ActiveDialog | null;
  open: (req: DialogRequest) => Promise<Answer>;
  close: (value: Answer) => void;
}

const cancelValue = (kind: DialogRequest['kind']): Answer => (kind === 'prompt' ? null : false);
let nextId = 1;

export const useDialogStore = create<DialogState>((set, get) => ({
  current: null,
  open: (req) =>
    new Promise<Answer>((resolve) => {
      // A dialog already up is answered as cancelled so the new request isn't lost.
      get().current?.resolve(cancelValue(get().current!.kind));
      set({ current: { ...req, id: nextId++, resolve } });
    }),
  close: (value) => {
    const cur = get().current;
    set({ current: null });
    cur?.resolve(value);
  },
}));

export function appConfirm(opts: Omit<DialogRequest, 'kind'>): Promise<boolean> {
  return useDialogStore.getState().open({ ...opts, kind: 'confirm' }) as Promise<boolean>;
}

export function appAlert(opts: Omit<DialogRequest, 'kind' | 'cancelLabel' | 'danger'>): Promise<void> {
  return useDialogStore.getState().open({ ...opts, kind: 'alert' }).then(() => undefined);
}

/** Resolves to the entered text, or null when dismissed. */
export function appPrompt(opts: Omit<DialogRequest, 'kind'>): Promise<string | null> {
  return useDialogStore.getState().open({ ...opts, kind: 'prompt' }) as Promise<string | null>;
}
