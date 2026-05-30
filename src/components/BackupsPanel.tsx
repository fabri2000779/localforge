/**
 * Server-level backup OPERATIONS.
 *
 * Storage configuration (which S3 bucket / credentials) lives in
 * Settings → Backup Storage. Here the user just picks WHICH of the org's
 * configured targets to use, then runs backups, restores, and deletes.
 *
 * If no storage is configured at all, an inline CTA links to Settings.
 */
import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useNavigate } from 'react-router-dom';
import {
  Archive, HardDriveDownload, RotateCcw, Trash2, Loader2,
  AlertTriangle, ChevronDown, Settings,
} from 'lucide-react';

interface OrgBackupTargetView {
  id: string;
  name: string;
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
  let v = bytes / 1024; let i = 0;
  while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
  return `${v.toFixed(1)} ${u[i]!}`;
}
function fmtDate(ms: number): string {
  return ms ? new Date(ms).toLocaleString() : '—';
}
function shortKey(key: string): string {
  return key.split('/').pop() || key;
}

export function BackupsPanel({ serverId, nodeId }: { serverId: string; nodeId: string }) {
  const navigate = useNavigate();

  const [targets, setTargets] = useState<OrgBackupTargetView[] | null>(null); // null = loading
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [backups, setBackups] = useState<BackupEntry[]>([]);
  const [listing, setListing] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);

  // ── load targets ─────────────────────────────────────────────────────────

  const loadTargets = useCallback(async () => {
    try {
      const list = await invoke<OrgBackupTargetView[]>('cloud_pull_backup_targets');
      setTargets(list);
      if (!selectedId && list.length > 0) setSelectedId(list[0]!.id);
    } catch (e) {
      // Fall back to locally cached list on cloud error.
      try {
        const list = await invoke<OrgBackupTargetView[]>('cloud_list_backup_targets');
        setTargets(list);
        if (!selectedId && list.length > 0) setSelectedId(list[0]!.id);
      } catch {
        setTargets([]);
      }
    }
  }, [selectedId]);

  useEffect(() => { void loadTargets(); }, [loadTargets]);

  // ── backup list ───────────────────────────────────────────────────────────

  const refreshList = useCallback(async () => {
    if (!selectedId) return;
    setListing(true); setErr(null);
    try {
      const list = await invoke<BackupEntry[]>('cloud_list_backups', {
        serverId, nodeId, targetId: selectedId,
      });
      setBackups(list);
    } catch (e) {
      setErr(String(e));
    } finally {
      setListing(false);
    }
  }, [serverId, nodeId, selectedId]);

  useEffect(() => { if (selectedId) void refreshList(); }, [refreshList, selectedId]);

  // ── actions ───────────────────────────────────────────────────────────────

  async function backupNow() {
    setBusy('backup'); setErr(null); setNote(null);
    try {
      await invoke<string>('cloud_backup_now', { serverId, nodeId, targetId: selectedId });
      setNote('Backup uploaded successfully.');
      await refreshList();
    } catch (e) { setErr(String(e)); }
    finally { setBusy(null); }
  }

  async function restore(key: string) {
    if (!confirm('Restore this backup? The server will be stopped and its current data moved aside before the backup is extracted.')) return;
    setBusy(`restore:${key}`); setErr(null); setNote(null);
    try {
      await invoke('cloud_restore_backup', { serverId, key, nodeId, targetId: selectedId });
      setNote('Restored. Start the server to load the restored data.');
    } catch (e) { setErr(String(e)); }
    finally { setBusy(null); }
  }

  async function del(key: string) {
    if (!confirm('Delete this backup from the bucket? This cannot be undone.')) return;
    setBusy(`del:${key}`); setErr(null); setNote(null);
    try {
      await invoke('cloud_delete_backup', { key, nodeId, targetId: selectedId });
      setBackups((b) => b.filter((x) => x.key !== key));
    } catch (e) { setErr(String(e)); }
    finally { setBusy(null); }
  }

  // ── render ────────────────────────────────────────────────────────────────

  if (targets === null) {
    return (
      <div className="card flex items-center gap-3 py-10 text-zinc-400">
        <Loader2 size={18} className="animate-spin" /> Loading…
      </div>
    );
  }

  // No storage configured at all — direct to Settings.
  if (targets.length === 0) {
    return (
      <div className="card text-center space-y-4 py-10">
        <Archive size={36} className="mx-auto text-zinc-600" />
        <div className="space-y-1">
          <p className="font-medium text-zinc-200">No backup storage configured</p>
          <p className="text-sm text-zinc-500">
            Add an S3-compatible bucket in Settings to start backing up this server.
          </p>
        </div>
        <button
          className="btn btn-primary mx-auto"
          onClick={() => navigate('/settings?tab=backup-storage')}
        >
          <Settings size={15} /> Configure backup storage
        </button>
      </div>
    );
  }

  const selected = targets.find((t) => t.id === selectedId);

  return (
    <div className="space-y-4">
      {/* Storage selector + action */}
      <div className="card">
        <div className="flex items-center justify-between gap-3 mb-3">
          <div className="flex items-center gap-2 font-medium text-zinc-200">
            <Archive size={16} className="text-sky-400" /> Backup storage
          </div>
          <button
            className="btn btn-ghost btn-sm text-zinc-500 hover:text-zinc-300"
            onClick={() => navigate('/settings?tab=backup-storage')}
            title="Manage backup storage targets"
          >
            <Settings size={13} /> Manage
          </button>
        </div>

        {/* Target selector — shown only when multiple exist */}
        {targets.length > 1 && (
          <div className="relative mb-3">
            <select
              className="input w-full appearance-none pr-8"
              value={selectedId ?? ''}
              onChange={(e) => setSelectedId(e.target.value)}
            >
              {targets.map((t) => (
                <option key={t.id} value={t.id}>{t.name} — {t.bucket}</option>
              ))}
            </select>
            <ChevronDown size={14} className="absolute right-3 top-1/2 -translate-y-1/2 text-zinc-400 pointer-events-none" />
          </div>
        )}

        {/* Selected target summary */}
        {selected && (
          <div className="text-sm text-zinc-400 space-y-0.5 mb-4">
            {targets.length === 1 && <div className="font-medium text-zinc-300 mb-1">{selected.name}</div>}
            <div><span className="text-zinc-500">Bucket:</span> <span className="font-mono text-zinc-300">{selected.bucket}</span></div>
            <div><span className="text-zinc-500">Endpoint:</span> <span className="font-mono text-zinc-300">{selected.endpoint || 'AWS S3 default'}</span></div>
          </div>
        )}

        <div className="flex items-center gap-3">
          <button
            className="btn btn-primary"
            onClick={backupNow}
            disabled={busy === 'backup' || !selectedId}
          >
            {busy === 'backup'
              ? <Loader2 size={15} className="animate-spin" />
              : <HardDriveDownload size={15} />}
            {busy === 'backup' ? 'Backing up…' : 'Back up now'}
          </button>
          <span className="text-xs text-zinc-500">
            Tip: stop the server first for a fully consistent snapshot.
          </span>
        </div>
      </div>

      {err && (
        <div className="card flex items-start gap-2 text-sm text-red-300 border-red-500/30">
          <AlertTriangle size={15} className="mt-0.5 shrink-0" />
          <span className="break-all">{err}</span>
        </div>
      )}
      {note && <div className="text-sm text-emerald-400 px-1">{note}</div>}

      {/* Backups list */}
      <div className="card">
        <div className="flex items-center justify-between mb-3">
          <span className="font-medium text-zinc-200">
            Backups {selected && targets.length > 1 && <span className="text-zinc-500 font-normal">in {selected.name}</span>}
          </span>
          <button className="btn btn-secondary btn-sm" onClick={refreshList} disabled={listing || !selectedId}>
            <RotateCcw size={13} className={listing ? 'animate-spin' : ''} /> Refresh
          </button>
        </div>
        {backups.length === 0 ? (
          <p className="text-sm text-zinc-500 py-3">
            {listing ? 'Loading…' : 'No backups yet. Click "Back up now" to create one.'}
          </p>
        ) : (
          <div className="divide-y divide-zinc-800">
            {backups.map((b) => (
              <div key={b.key} className="flex items-center gap-3 py-2.5">
                <div className="min-w-0 flex-1">
                  <div className="font-mono text-[13px] text-zinc-200 truncate">{shortKey(b.key)}</div>
                  <div className="text-xs text-zinc-500">{fmtDate(b.createdAt)} · {fmtSize(b.size)}</div>
                </div>
                <button
                  className="btn btn-secondary btn-sm"
                  onClick={() => restore(b.key)}
                  disabled={!!busy}
                >
                  {busy === `restore:${b.key}` ? <Loader2 size={13} className="animate-spin" /> : <RotateCcw size={13} />}
                  Restore
                </button>
                <button
                  className="icon-btn text-zinc-400 hover:text-red-400"
                  title="Delete backup"
                  onClick={() => del(b.key)}
                  disabled={!!busy}
                >
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
