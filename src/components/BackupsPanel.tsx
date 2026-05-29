/**
 * Backups tab — bring-your-own-S3 backups for a server.
 *
 * The user configures one S3-compatible destination (R2 / B2 / Wasabi / MinIO /
 * DO Spaces…); the secret lives in the OS keychain and is never shown back.
 * Backups archive the server's data dir → the bucket; restore pulls one back
 * (stopping the server first). Works for local AND agent-hosted servers — the
 * `nodeId` routes the call to the right host's backend.
 */
import { useCallback, useEffect, useState, type FormEvent } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
  Cloud, Save, Loader2, RotateCcw, Trash2, HardDriveDownload, AlertTriangle, Pencil,
} from 'lucide-react';

interface BackupTargetView {
  endpoint: string;
  region: string;
  bucket: string;
  accessKey: string;
  pathStyle: boolean;
}
interface BackupEntry {
  key: string;
  size: number;
  createdAt: number;
}

function fmtSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const u = ['KB', 'MB', 'GB', 'TB'];
  let v = bytes / 1024;
  let i = 0;
  while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
  return `${v.toFixed(1)} ${u[i]}`;
}
function fmtDate(ms: number): string {
  if (!ms) return '—';
  return new Date(ms).toLocaleString();
}
function shortKey(key: string): string {
  return key.split('/').pop() || key;
}

export function BackupsPanel({ serverId, nodeId }: { serverId: string; nodeId: string }) {
  // undefined = loading, null = not configured, object = configured.
  const [target, setTarget] = useState<BackupTargetView | null | undefined>(undefined);
  const [editing, setEditing] = useState(false);
  const [backups, setBackups] = useState<BackupEntry[]>([]);
  const [listing, setListing] = useState(false);
  const [busy, setBusy] = useState<string | null>(null); // active action key
  const [err, setErr] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);

  const refreshList = useCallback(async () => {
    setListing(true);
    setErr(null);
    try {
      const list = await invoke<BackupEntry[]>('cloud_list_backups', { serverId, nodeId });
      setBackups(list);
    } catch (e) {
      setErr(String(e));
    } finally {
      setListing(false);
    }
  }, [serverId, nodeId]);

  const loadTarget = useCallback(async () => {
    try {
      // Pull the org-shared target from the cloud first (hydrates this device's
      // keychain + provisions agents), falling back to whatever's local. So a
      // fresh device picks up the org's backup storage without re-entering it.
      const t = await invoke<BackupTargetView | null>('cloud_pull_backup_target');
      setTarget(t);
      if (t) void refreshList();
    } catch (e) {
      setErr(String(e));
      setTarget(null);
    }
  }, [refreshList]);

  useEffect(() => { void loadTarget(); }, [loadTarget]);

  async function backupNow() {
    setBusy('backup'); setErr(null); setNote(null);
    try {
      await invoke<string>('cloud_backup_now', { serverId, nodeId });
      setNote('Backup uploaded.');
      await refreshList();
    } catch (e) { setErr(String(e)); }
    finally { setBusy(null); }
  }

  async function restore(key: string) {
    if (!confirm('Restore this backup? The server will be stopped and its current data moved aside before the backup is extracted.')) return;
    setBusy(`restore:${key}`); setErr(null); setNote(null);
    try {
      await invoke('cloud_restore_backup', { serverId, key, nodeId });
      setNote('Restored. Start the server to load the restored data.');
    } catch (e) { setErr(String(e)); }
    finally { setBusy(null); }
  }

  async function del(key: string) {
    if (!confirm('Delete this backup from the bucket? This cannot be undone.')) return;
    setBusy(`del:${key}`); setErr(null); setNote(null);
    try {
      await invoke('cloud_delete_backup', { key, nodeId });
      setBackups((b) => b.filter((x) => x.key !== key));
    } catch (e) { setErr(String(e)); }
    finally { setBusy(null); }
  }

  if (target === undefined) {
    return (
      <div className="card flex items-center justify-center gap-3 py-10 text-zinc-400">
        <Loader2 size={18} className="animate-spin" /> Loading backups…
      </div>
    );
  }

  if (target === null || editing) {
    return (
      <TargetForm
        initial={target ?? undefined}
        onCancel={target ? () => setEditing(false) : undefined}
        onSaved={async () => { setEditing(false); await loadTarget(); }}
      />
    );
  }

  return (
    <div className="space-y-4">
      {/* Destination */}
      <div className="card">
        <div className="flex items-center justify-between gap-3 mb-3">
          <div className="flex items-center gap-2 font-medium text-zinc-200">
            <Cloud size={16} className="text-sky-400" /> Backup destination
          </div>
          <button className="btn btn-secondary btn-sm" onClick={() => setEditing(true)}>
            <Pencil size={13} /> Change
          </button>
        </div>
        <div className="text-sm text-zinc-400 space-y-1">
          <div><span className="text-zinc-500">Bucket:</span> <span className="font-mono text-zinc-300">{target.bucket}</span></div>
          <div><span className="text-zinc-500">Endpoint:</span> <span className="font-mono text-zinc-300">{target.endpoint || 'AWS default'}</span></div>
          <div><span className="text-zinc-500">Region:</span> <span className="font-mono text-zinc-300">{target.region || '—'}</span></div>
        </div>
        <div className="flex items-center gap-2 mt-4">
          <button className="btn btn-primary" onClick={backupNow} disabled={busy === 'backup'}>
            {busy === 'backup' ? <Loader2 size={15} className="animate-spin" /> : <HardDriveDownload size={15} />}
            {busy === 'backup' ? 'Backing up…' : 'Back up now'}
          </button>
          <span className="text-xs text-zinc-500">Tip: stop the server first for a fully consistent snapshot.</span>
        </div>
      </div>

      {err && (
        <div className="card flex items-start gap-2 text-sm text-red-300 border-red-500/30">
          <AlertTriangle size={15} className="mt-0.5 shrink-0" /> <span className="break-all">{err}</span>
        </div>
      )}
      {note && <div className="text-sm text-emerald-400 px-1">{note}</div>}

      {/* Backups list */}
      <div className="card">
        <div className="flex items-center justify-between mb-3">
          <span className="font-medium text-zinc-200">Backups</span>
          <button className="btn btn-secondary btn-sm" onClick={refreshList} disabled={listing}>
            <RotateCcw size={13} className={listing ? 'animate-spin' : ''} /> Refresh
          </button>
        </div>
        {backups.length === 0 ? (
          <p className="text-sm text-zinc-500 py-3">{listing ? 'Loading…' : 'No backups yet. Click “Back up now”.'}</p>
        ) : (
          <div className="divide-y divide-zinc-800">
            {backups.map((b) => (
              <div key={b.key} className="flex items-center gap-3 py-2.5">
                <div className="min-w-0 flex-1">
                  <div className="font-mono text-[13px] text-zinc-200 truncate">{shortKey(b.key)}</div>
                  <div className="text-xs text-zinc-500">{fmtDate(b.createdAt)} · {fmtSize(b.size)}</div>
                </div>
                <button className="btn btn-secondary btn-sm" onClick={() => restore(b.key)} disabled={!!busy}>
                  {busy === `restore:${b.key}` ? <Loader2 size={13} className="animate-spin" /> : <RotateCcw size={13} />} Restore
                </button>
                <button className="icon-btn text-zinc-400 hover:text-red-400" title="Delete" onClick={() => del(b.key)} disabled={!!busy}>
                  {busy === `del:${b.key}` ? <Loader2 size={13} className="animate-spin" /> : <Trash2 size={14} />}
                </button>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function TargetForm({
  initial,
  onSaved,
  onCancel,
}: {
  initial?: BackupTargetView;
  onSaved: () => void;
  onCancel?: () => void;
}) {
  const [endpoint, setEndpoint] = useState(initial?.endpoint ?? '');
  const [region, setRegion] = useState(initial?.region ?? 'auto');
  const [bucket, setBucket] = useState(initial?.bucket ?? '');
  const [accessKey, setAccessKey] = useState(initial?.accessKey ?? '');
  const [secretKey, setSecretKey] = useState('');
  const [pathStyle, setPathStyle] = useState(initial?.pathStyle ?? false);
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    setSaving(true); setErr(null);
    try {
      await invoke('cloud_set_backup_target', {
        target: { endpoint: endpoint.trim(), region: region.trim(), bucket: bucket.trim(), accessKey: accessKey.trim(), secretKey, pathStyle },
      });
      onSaved();
    } catch (e) { setErr(String(e)); }
    finally { setSaving(false); }
  }

  return (
    <form className="card space-y-3" onSubmit={onSubmit}>
      <div className="flex items-center gap-2 font-medium text-zinc-200">
        <Cloud size={16} className="text-sky-400" /> S3 backup destination
      </div>
      <p className="text-xs text-zinc-500">
        Use your own S3-compatible bucket (Cloudflare R2, Backblaze B2, Wasabi, MinIO, DigitalOcean Spaces…).
        Your secret key is stored in the OS keychain and never leaves this device in plaintext.
      </p>
      <Field label="Bucket" value={bucket} onChange={setBucket} placeholder="my-localforge-backups" required />
      <Field label="Endpoint URL" value={endpoint} onChange={setEndpoint} placeholder="https://<account>.r2.cloudflarestorage.com" />
      <Field label="Region" value={region} onChange={setRegion} placeholder="auto / us-east-1" />
      <Field label="Access key ID" value={accessKey} onChange={setAccessKey} placeholder="AKIA… / R2 token id" required />
      <Field label="Secret access key" value={secretKey} onChange={setSecretKey} placeholder={initial ? '•••••• (unchanged — re-enter to update)' : ''} type="password" required={!initial} />
      <label className="flex items-center gap-2 text-sm text-zinc-300">
        <input type="checkbox" checked={pathStyle} onChange={(e) => setPathStyle(e.target.checked)} />
        Path-style addressing (MinIO and some S3-compatible providers)
      </label>
      {err && <div className="text-sm text-red-300 break-all">{err}</div>}
      <div className="flex items-center gap-2">
        <button type="submit" className="btn btn-primary" disabled={saving || !bucket.trim() || !accessKey.trim()}>
          {saving ? <Loader2 size={15} className="animate-spin" /> : <Save size={15} />} Save destination
        </button>
        {onCancel && <button type="button" className="btn btn-secondary" onClick={onCancel} disabled={saving}>Cancel</button>}
      </div>
    </form>
  );
}

function Field({
  label, value, onChange, placeholder, type = 'text', required = false,
}: {
  label: string; value: string; onChange: (v: string) => void;
  placeholder?: string; type?: string; required?: boolean;
}) {
  return (
    <label className="block">
      <span className="text-[11px] uppercase tracking-wider font-semibold text-zinc-500">{label}</span>
      <input
        type={type}
        className="input w-full mt-1"
        value={value}
        placeholder={placeholder}
        required={required}
        autoComplete="off"
        autoCapitalize="off"
        spellCheck={false}
        onChange={(e) => onChange(e.target.value)}
      />
    </label>
  );
}
