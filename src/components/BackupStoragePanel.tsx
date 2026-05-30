/**
 * Org-level backup storage management — lives in Settings, not per-server.
 *
 * The S3 credentials are an org resource (shared across all servers and all
 * devices in the org). This panel lets the owner/admin add, view, and remove
 * named storage targets. Each target's credentials are stored in the OS
 * keychain (locally) and synced E2E encrypted to the cloud so other devices
 * pick them up automatically.
 *
 * Per-server backup OPERATIONS (back up now / restore / delete) live in the
 * server's Backups tab — this panel is purely about managing WHERE backups go.
 */
import { useCallback, useEffect, useState, type FormEvent } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
  Archive, Plus, Trash2, ChevronDown, ChevronUp,
  Loader2, Save, Check,
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

// ── helpers ────────────────────────────────────────────────────────────────

function Field({
  label, value, onChange, placeholder, type = 'text', required = false,
}: {
  label: string; value: string; onChange: (v: string) => void;
  placeholder?: string; type?: string; required?: boolean;
}) {
  return (
    <label className="block">
      <span className="text-[11px] uppercase tracking-wider font-semibold text-zinc-500">
        {label}
      </span>
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

// ── add form ───────────────────────────────────────────────────────────────

function AddTargetForm({ onSaved, onCancel }: { onSaved: (t: OrgBackupTargetView) => void; onCancel: () => void }) {
  const [name, setName] = useState('');
  const [endpoint, setEndpoint] = useState('');
  const [region, setRegion] = useState('auto');
  const [bucket, setBucket] = useState('');
  const [accessKey, setAccessKey] = useState('');
  const [secretKey, setSecretKey] = useState('');
  const [pathStyle, setPathStyle] = useState(false);
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    setSaving(true); setErr(null);
    try {
      const id = crypto.randomUUID();
      const view = await invoke<OrgBackupTargetView>('cloud_add_backup_target', {
        target: {
          id,
          name: name.trim(),
          credentials: {
            endpoint: endpoint.trim(),
            region: region.trim(),
            bucket: bucket.trim(),
            accessKey: accessKey.trim(),
            secretKey,
            pathStyle,
          },
        },
      });
      onSaved(view);
    } catch (e) {
      setErr(String(e));
    } finally {
      setSaving(false);
    }
  }

  return (
    <form className="card space-y-3 border border-sky-500/20" onSubmit={onSubmit}>
      <div className="font-medium text-zinc-200 flex items-center gap-2">
        <Archive size={15} className="text-sky-400" /> New backup storage
      </div>
      <p className="text-xs text-zinc-500">
        Compatible with Cloudflare R2, Backblaze B2, Wasabi, MinIO, DigitalOcean Spaces, and any S3-compatible bucket.
        The secret key is stored in the OS keychain and synced E2E-encrypted to your org — it never leaves a device in plaintext.
      </p>
      <Field label="Name" value={name} onChange={setName} placeholder="e.g. Cloudflare R2 Production" required />
      <Field label="Bucket" value={bucket} onChange={setBucket} placeholder="my-localforge-backups" required />
      <Field label="Endpoint URL" value={endpoint} onChange={setEndpoint} placeholder="https://<account>.r2.cloudflarestorage.com  (empty = AWS S3)" />
      <Field label="Region" value={region} onChange={setRegion} placeholder="auto / us-east-1" />
      <Field label="Access key ID" value={accessKey} onChange={setAccessKey} placeholder="AKIA… / R2 token id" required />
      <Field label="Secret access key" value={secretKey} onChange={setSecretKey} type="password" required />
      <label className="flex items-center gap-2 text-sm text-zinc-300 cursor-pointer">
        <input type="checkbox" checked={pathStyle} onChange={(e) => setPathStyle(e.target.checked)} />
        Path-style addressing (MinIO and some self-hosted S3-compatible services)
      </label>
      {err && <div className="text-sm text-red-300 break-all">{err}</div>}
      <div className="flex items-center gap-2 pt-1">
        <button
          type="submit"
          className="btn btn-primary"
          disabled={saving || !bucket.trim() || !accessKey.trim() || !secretKey.trim() || !name.trim()}
        >
          {saving ? <Loader2 size={15} className="animate-spin" /> : <Save size={15} />}
          {saving ? 'Saving…' : 'Save storage'}
        </button>
        <button type="button" className="btn btn-secondary" onClick={onCancel} disabled={saving}>
          Cancel
        </button>
      </div>
    </form>
  );
}

// ── target row ─────────────────────────────────────────────────────────────

function TargetRow({ t, onRemoved }: { t: OrgBackupTargetView; onRemoved: (id: string) => void }) {
  const [expanded, setExpanded] = useState(false);
  const [removing, setRemoving] = useState(false);

  async function remove() {
    if (!confirm(`Remove backup storage "${t.name}"? Existing backups in the bucket are NOT deleted.`)) return;
    setRemoving(true);
    try {
      await invoke('cloud_remove_backup_target', { id: t.id });
      onRemoved(t.id);
    } catch (e) {
      alert(String(e));
      setRemoving(false);
    }
  }

  return (
    <div className="border border-zinc-700/60 rounded-xl overflow-hidden">
      <div
        className="flex items-center gap-3 px-4 py-3 cursor-pointer select-none hover:bg-zinc-800/40 transition-colors"
        onClick={() => setExpanded((e) => !e)}
      >
        <Archive size={15} className="text-sky-400 shrink-0" />
        <div className="flex-1 min-w-0">
          <div className="font-medium text-zinc-200 text-sm">{t.name}</div>
          <div className="text-xs text-zinc-500 font-mono truncate">
            {t.bucket}{t.endpoint ? ` · ${t.endpoint}` : ' · AWS S3'}
          </div>
        </div>
        <div className="flex items-center gap-1 text-xs text-emerald-400">
          <Check size={12} /> Ready
        </div>
        <button
          className="icon-btn text-zinc-500 hover:text-red-400"
          title="Remove"
          onClick={(e) => { e.stopPropagation(); void remove(); }}
          disabled={removing}
        >
          {removing ? <Loader2 size={14} className="animate-spin" /> : <Trash2 size={14} />}
        </button>
        {expanded ? <ChevronUp size={14} className="text-zinc-500" /> : <ChevronDown size={14} className="text-zinc-500" />}
      </div>
      {expanded && (
        <div className="px-4 pb-3 pt-0 space-y-1 text-[13px] text-zinc-400 border-t border-zinc-800">
          <div className="mt-2"><span className="text-zinc-500">Bucket:</span> <span className="font-mono text-zinc-300">{t.bucket}</span></div>
          <div><span className="text-zinc-500">Endpoint:</span> <span className="font-mono text-zinc-300">{t.endpoint || 'AWS S3 default'}</span></div>
          <div><span className="text-zinc-500">Region:</span> <span className="font-mono text-zinc-300">{t.region || 'auto'}</span></div>
          <div><span className="text-zinc-500">Access key:</span> <span className="font-mono text-zinc-300">{t.accessKey}</span></div>
          <div><span className="text-zinc-500">Path-style:</span> <span className="text-zinc-300">{t.pathStyle ? 'Yes' : 'No'}</span></div>
          <div><span className="text-zinc-500">Secret key:</span> <span className="text-zinc-500">••••••••  (stored in OS keychain)</span></div>
        </div>
      )}
    </div>
  );
}

// ── main panel ─────────────────────────────────────────────────────────────

export function BackupStoragePanel() {
  const [targets, setTargets] = useState<OrgBackupTargetView[] | null>(null);
  const [adding, setAdding] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const load = useCallback(async () => {
    setErr(null);
    try {
      // Pull from cloud first (syncs other devices' targets locally too).
      const list = await invoke<OrgBackupTargetView[]>('cloud_pull_backup_targets');
      setTargets(list);
    } catch (e) {
      setErr(String(e));
      // Fall back to locally cached list.
      try {
        setTargets(await invoke<OrgBackupTargetView[]>('cloud_list_backup_targets'));
      } catch { setTargets([]); }
    }
  }, []);

  useEffect(() => { void load(); }, [load]);

  function handleAdded(t: OrgBackupTargetView) {
    setTargets((prev) => [...(prev ?? []), t]);
    setAdding(false);
  }

  function handleRemoved(id: string) {
    setTargets((prev) => (prev ?? []).filter((t) => t.id !== id));
  }

  if (targets === null) {
    return (
      <section className="card flex items-center gap-3 py-10 text-zinc-400">
        <Loader2 size={16} className="animate-spin" /> Loading backup storage…
      </section>
    );
  }

  return (
    <div className="space-y-4">
      <section className="card">
        <div className="section-header mb-3">
          <div className="section-title">
            <Archive size={15} className="text-sky-400" />
            Backup storage targets
          </div>
          {!adding && (
            <button className="btn btn-secondary btn-sm" onClick={() => setAdding(true)}>
              <Plus size={13} /> Add storage
            </button>
          )}
        </div>
        <p className="text-xs text-zinc-500 mb-4">
          Org-shared S3-compatible destinations. Any server in your org can back up to any of these.
          Credentials are encrypted with your org key before leaving this device — LocalForge never sees them in plaintext.
        </p>
        {err && <div className="text-sm text-amber-300 mb-3 break-all">{err}</div>}

        {targets.length === 0 && !adding ? (
          <div className="text-center py-8 space-y-3">
            <Archive size={32} className="mx-auto text-zinc-600" />
            <p className="text-sm text-zinc-500">No backup storage configured yet.</p>
            <button className="btn btn-primary" onClick={() => setAdding(true)}>
              <Plus size={15} /> Add your first storage
            </button>
          </div>
        ) : (
          <div className="space-y-2">
            {targets.map((t) => (
              <TargetRow key={t.id} t={t} onRemoved={handleRemoved} />
            ))}
          </div>
        )}
      </section>

      {adding && (
        <AddTargetForm
          onSaved={handleAdded}
          onCancel={() => setAdding(false)}
        />
      )}
    </div>
  );
}
