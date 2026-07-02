import { useEffect } from 'react';

/**
 * Close a modal on Escape. Shared by every dialog in the app so keyboard
 * users aren't trapped (audit finding: no dialog closed on Escape).
 *
 * `enabled` lets a modal opt out while a nested confirm is up or an async
 * action is in flight.
 */
export function useEscapeClose(onClose: () => void, enabled = true) {
  useEffect(() => {
    if (!enabled) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose, enabled]);
}
