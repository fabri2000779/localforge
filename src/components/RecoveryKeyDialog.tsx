/**
 * Two modes:
 *   "show"   — display the local vault key so the user can copy it to
 *              a password manager / second device.
 *   "import" — paste an existing key from another device; replaces the
 *              local one. Used to set up cloud sync on a second install.
 *
 * The key is a 256-bit AES-GCM key (the DEK) base64-encoded. The cloud
 * never sees it in the clear — only a copy WRAPPED behind the user's
 * password/passphrase (see cloud_sync_key_setup) so other devices can
 * unlock it. This dialog handles the raw key for manual device-to-device
 * transfer.
 */
import { useEffect, useState } from 'react';
import { X, Copy, ShieldCheck, AlertTriangle } from 'lucide-react';
import { useAuthStore } from '../stores/authStore';

interface Props {
  open: boolean;
  mode: 'show' | 'import';
  onClose: () => void;
}

export function RecoveryKeyDialog({ open, mode, onClose }: Props) {
  const exportKey = useAuthStore((s) => s.vaultExportKey);
  const importKey = useAuthStore((s) => s.vaultImportKey);

  const [keyText, setKeyText] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (!open) {
      setKeyText('');
      setError(null);
      setCopied(false);
      return;
    }
    if (mode === 'show') {
      void exportKey().then((k) => setKeyText(k ?? ''));
    }
  }, [open, mode, exportKey]);

  if (!open) return null;

  async function onCopy() {
    try {
      await navigator.clipboard.writeText(keyText);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (e) {
      setError(String(e));
    }
  }

  async function onImport() {
    setError(null);
    const ok = await importKey(keyText.trim());
    if (ok) {
      onClose();
    } else {
      setError('That key isn’t valid. Make sure you copied the whole thing.');
    }
  }

  return (
    <div className="auth-overlay" onClick={onClose} role="dialog" aria-modal="true">
      <div className="auth-modal" onClick={(e) => e.stopPropagation()} style={{ maxWidth: 480 }}>
        <button className="auth-close" onClick={onClose} aria-label="Close">
          <X size={16} strokeWidth={2.2} />
        </button>
        <div className="auth-header">
          <h2>{mode === 'show' ? 'Your recovery key' : 'Restore from a recovery key'}</h2>
          <p>
            {mode === 'show'
              ? "This is what we use to encrypt the configs we sync. The cloud can't read them without it. Store this somewhere safe — losing it means losing access to your cloud-synced configs."
              : "Paste the recovery key from your other device. We'll use it to decrypt your existing cloud data here."}
          </p>
        </div>

        <div className="vault-key-block">
          <textarea
            value={keyText}
            onChange={(e) => setKeyText(e.target.value)}
            spellCheck={false}
            readOnly={mode === 'show'}
            placeholder={mode === 'import' ? 'Paste your recovery key here…' : ''}
            rows={3}
          />
          {mode === 'show' && (
            <button className="btn-secondary" onClick={onCopy} disabled={!keyText}>
              <Copy size={13} strokeWidth={2.2} />
              {copied ? 'Copied' : 'Copy'}
            </button>
          )}
        </div>

        {mode === 'show' && (
          <div className="vault-warn">
            <ShieldCheck size={14} className="shrink-0 text-emerald-400 mt-[2px]" />
            <span>Treat this like a password — anyone who has it can read your synced configs. The cloud only ever stores it encrypted behind your account password (or sync passphrase), so we can't.</span>
          </div>
        )}
        {mode === 'import' && (
          <div className="vault-warn vault-warn-yellow">
            <AlertTriangle size={14} className="shrink-0 text-amber-400 mt-[2px]" />
            <span>This replaces your current local key. If you have unsynced local configs, push them first.</span>
          </div>
        )}

        {error && <div className="auth-err mt-2">{error}</div>}

        {mode === 'import' && (
          <button className="auth-submit" disabled={!keyText.trim()} onClick={onImport}>
            Restore key
          </button>
        )}
      </div>
    </div>
  );
}
