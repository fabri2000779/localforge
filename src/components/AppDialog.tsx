/** Host for the promise-based app dialogs (see dialogStore); mounted once in App. */
import { useState } from 'react';
import { createPortal } from 'react-dom';
import { AlertTriangle, X } from 'lucide-react';
import { useDialogStore, type ActiveDialog } from '../stores/dialogStore';
import { useEscapeClose } from '../hooks/useEscapeClose';

export function AppDialog() {
  const current = useDialogStore((s) => s.current);
  const close = useDialogStore((s) => s.close);
  if (!current) return null;
  // Keyed by request id so the prompt text resets for every new dialog.
  return <DialogBody key={current.id} req={current} close={close} />;
}

function DialogBody({ req, close }: { req: ActiveDialog; close: (v: boolean | string | null) => void }) {
  const [value, setValue] = useState(req.defaultValue ?? '');
  const isPrompt = req.kind === 'prompt';
  const canSubmit = !isPrompt || req.allowEmpty || value.trim().length > 0;
  const cancel = () => close(isPrompt ? null : false);
  const submit = () => {
    if (!canSubmit) return;
    close(isPrompt ? value : true);
  };
  useEscapeClose(cancel);

  return createPortal(
    <div className="auth-overlay" onClick={cancel} role="dialog" aria-modal="true" aria-labelledby="app-dialog-title">
      <div className="auth-modal" onClick={(e) => e.stopPropagation()} style={{ maxWidth: 440 }}>
        <button className="auth-close" onClick={cancel} aria-label="Close">
          <X size={16} strokeWidth={2.2} />
        </button>
        <div className="auth-header">
          <h2 id="app-dialog-title">
            {req.danger && (
              <AlertTriangle size={16} className="text-red-400" style={{ display: 'inline', marginRight: 6, verticalAlign: '-2px' }} />
            )}
            {req.title}
          </h2>
          {req.message && <p>{req.message}</p>}
        </div>

        {isPrompt && (
          <label className="block text-[13px] text-zinc-400">
            {req.label && <span className="block mb-1">{req.label}</span>}
            <input
              className="input w-full"
              autoFocus
              value={value}
              placeholder={req.placeholder}
              onChange={(e) => setValue(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') submit();
              }}
            />
          </label>
        )}

        <div style={{ display: 'flex', gap: 8, marginTop: 14 }}>
          <button
            className={req.danger ? 'btn btn-danger' : 'auth-submit'}
            style={{ flex: 1 }}
            autoFocus={!isPrompt}
            onClick={submit}
            disabled={!canSubmit}
          >
            {req.confirmLabel ?? (req.kind === 'alert' ? 'OK' : 'Confirm')}
          </button>
          {req.kind !== 'alert' && (
            <button className="btn btn-secondary" onClick={cancel}>
              {req.cancelLabel ?? 'Cancel'}
            </button>
          )}
        </div>
      </div>
    </div>,
    document.body,
  );
}
