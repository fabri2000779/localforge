/**
 * Schedules tab — cron-scheduled actions for a server.
 *
 * The host's scheduler loop (desktop while open, agent 24/7) fires these. The
 * cron expression is standard 5-field (`min hour dom month dow`) in the host's
 * local time. Actions: restart, run a console command, or broadcast a message.
 */
import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
  Clock, Plus, Trash2, Loader2, Power, PowerOff, RotateCcw, Terminal, Megaphone, Archive, Check, X,
} from 'lucide-react';

// Wire shape of core::ScheduleAction. NB: the Rust enum's rename_all =
// "camelCase" renames only the VARIANT TAGS — struct-variant FIELDS stay
// snake_case. Sending camelCase fields here made serde silently drop the
// backup target + retention on every schedule created from the UI (audit
// finding: wrong bucket + unbounded growth).
type ScheduleAction =
  | { kind: 'restart' }
  | { kind: 'command'; command: string }
  | { kind: 'broadcast'; message: string }
  | { kind: 'backup'; target_id?: string; keep_last?: number; max_age_days?: number };

/** Minimal projection of OrgBackupTargetView — just what the picker needs. */
interface BackupTargetOption { id: string; name: string }

interface Schedule {
  id: string;
  serverId: string;
  cron: string;
  action: ScheduleAction;
  enabled: boolean;
  lastRun?: number | null;
}

const CRON_PRESETS: Array<{ label: string; cron: string }> = [
  { label: 'Daily · 5:00 AM', cron: '0 5 * * *' },
  { label: 'Every 6 hours', cron: '0 */6 * * *' },
  { label: 'Every 12 hours', cron: '0 */12 * * *' },
  { label: 'Weekly · Sun 4 AM', cron: '0 4 * * 0' },
];

function actionLabel(a: ScheduleAction): string {
  if (a.kind === 'restart') return 'Restart';
  if (a.kind === 'command') return `Command: ${a.command}`;
  if (a.kind === 'backup') return 'Backup to S3';
  return `Broadcast: ${a.message}`;
}
function ActionIcon({ kind }: { kind: ScheduleAction['kind'] }) {
  if (kind === 'restart') return <RotateCcw size={15} className="text-amber-400" />;
  if (kind === 'command') return <Terminal size={15} className="text-sky-400" />;
  if (kind === 'backup') return <Archive size={15} className="text-emerald-400" />;
  return <Megaphone size={15} className="text-sky-400" />;
}

export function SchedulesPanel({ serverId, nodeId }: { serverId: string; nodeId: string }) {
  const [schedules, setSchedules] = useState<Schedule[]>([]);
  const [loading, setLoading] = useState(true);
  const [adding, setAdding] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true); setErr(null);
    try {
      setSchedules(await invoke<Schedule[]>('list_schedules', { serverId, nodeId }));
    } catch (e) { setErr(String(e)); }
    finally { setLoading(false); }
  }, [serverId, nodeId]);

  useEffect(() => { void refresh(); }, [refresh]);

  // `busyKey` lets the create form register as 'new' — the form's saving
  // prop checks busy === 'new', but save() used to set busy to the fresh
  // UUID, so the flag never matched and a double-click created duplicate
  // schedules (audit finding).
  async function save(s: Schedule, busyKey: string = s.id) {
    setBusy(busyKey); setErr(null);
    try {
      await invoke('upsert_schedule', { schedule: s, nodeId });
      await refresh();
      setAdding(false);
    } catch (e) { setErr(String(e)); }
    finally { setBusy(null); }
  }

  async function toggle(s: Schedule) {
    await save({ ...s, enabled: !s.enabled });
  }

  async function remove(id: string) {
    if (!confirm('Delete this schedule?')) return;
    setBusy(id); setErr(null);
    try {
      await invoke('delete_schedule', { id, nodeId });
      setSchedules((cur) => cur.filter((x) => x.id !== id));
    } catch (e) { setErr(String(e)); }
    finally { setBusy(null); }
  }

  return (
    <div className="space-y-4">
      <div className="card">
        <div className="flex items-center justify-between mb-1">
          <div className="flex items-center gap-2 font-medium text-zinc-200">
            <Clock size={16} className="text-emerald-400" /> Scheduled actions
          </div>
          {!adding && (
            <button className="btn btn-secondary btn-sm" onClick={() => setAdding(true)}>
              <Plus size={13} /> New schedule
            </button>
          )}
        </div>
        <p className="text-xs text-zinc-500">
          Runs on the host (this desktop while open, or the agent 24/7). Times are the host's local time.
        </p>
      </div>

      {err && <div className="text-sm text-red-300 px-1 break-all">{err}</div>}

      {adding && (
        <ScheduleForm
          serverId={serverId}
          onCancel={() => setAdding(false)}
          onSave={(s) => save(s, 'new')}
          saving={busy === 'new'}
        />
      )}

      <div className="card">
        {loading ? (
          <div className="flex items-center gap-2 text-zinc-400 py-3"><Loader2 size={16} className="animate-spin" /> Loading…</div>
        ) : schedules.length === 0 ? (
          <p className="text-sm text-zinc-500 py-3">No schedules yet. Add one — e.g. a nightly restart.</p>
        ) : (
          <div className="divide-y divide-zinc-800">
            {schedules.map((s) => (
              <div key={s.id} className="flex items-center gap-3 py-2.5">
                <ActionIcon kind={s.action.kind} />
                <div className="min-w-0 flex-1">
                  <div className="text-[13px] text-zinc-200 truncate">{actionLabel(s.action)}</div>
                  <div className="text-xs text-zinc-500 font-mono">{s.cron}{s.lastRun ? ` · last ${new Date(s.lastRun).toLocaleString()}` : ''}</div>
                </div>
                <button
                  className={`inline-flex items-center gap-1 shrink-0 px-2 py-0.5 rounded-full border text-[11px] font-medium transition-colors ${
                    s.enabled
                      ? 'border-emerald-500/40 bg-emerald-500/10 text-emerald-300 hover:bg-emerald-500/20'
                      : 'border-zinc-700 bg-zinc-800/60 text-zinc-400 hover:text-zinc-200'
                  }`}
                  title={s.enabled ? 'Enabled — click to pause' : 'Paused — click to enable'}
                  aria-pressed={s.enabled}
                  onClick={() => toggle(s)}
                  disabled={busy === s.id}
                >
                  {busy === s.id ? (
                    <Loader2 size={13} className="animate-spin" />
                  ) : s.enabled ? (
                    <Power size={13} />
                  ) : (
                    <PowerOff size={13} />
                  )}
                  {s.enabled ? 'On' : 'Paused'}
                </button>
                <button className="icon-btn text-zinc-400 hover:text-red-400" title="Delete" onClick={() => remove(s.id)} disabled={busy === s.id}>
                  <Trash2 size={14} />
                </button>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function ScheduleForm({
  serverId, onSave, onCancel, saving,
}: {
  serverId: string;
  onSave: (s: Schedule) => void;
  onCancel: () => void;
  saving: boolean;
}) {
  const [kind, setKind] = useState<ScheduleAction['kind']>('restart');
  const [arg, setArg] = useState('');
  const [cron, setCron] = useState('0 5 * * *');
  const [targets, setTargets] = useState<BackupTargetOption[] | null>(null); // null = loading
  const [targetId, setTargetId] = useState<string>('');
  const [keepLast, setKeepLast] = useState<string>('7'); // retention floor (blank = no limit)
  const [maxAgeDays, setMaxAgeDays] = useState<string>(''); // age limit in days (blank = none)

  // Load the org's backup targets so the Backup action can offer a picker. The
  // local list is enough — ids match what the agent was provisioned with, so a
  // schedule resolves on whichever host (desktop or agent) actually fires it.
  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        const list = await invoke<BackupTargetOption[]>('cloud_list_backup_targets');
        if (!alive) return;
        setTargets(list);
        if (list.length > 0) setTargetId((cur) => cur || list[0].id);
      } catch {
        if (alive) setTargets([]);
      }
    })();
    return () => { alive = false; };
  }, []);

  function submit() {
    let action: ScheduleAction;
    if (kind === 'command') action = { kind: 'command', command: arg.trim() };
    else if (kind === 'broadcast') action = { kind: 'broadcast', message: arg.trim() };
    else if (kind === 'backup') {
      const kl = parseInt(keepLast, 10);
      const ma = parseInt(maxAgeDays, 10);
      action = {
        kind: 'backup',
        target_id: targetId || undefined,
        keep_last: Number.isFinite(kl) && kl > 0 ? kl : undefined,
        max_age_days: Number.isFinite(ma) && ma > 0 ? ma : undefined,
      };
    } else action = { kind: 'restart' };
    onSave({
      id: crypto.randomUUID(),
      serverId,
      cron: cron.trim(),
      action,
      enabled: true,
    });
  }

  const needsText = kind === 'command' || kind === 'broadcast';
  const cronValid = cron.trim().split(/\s+/).length === 5;
  const valid =
    cronValid &&
    (kind !== 'backup' ? true : (targets?.length ?? 0) > 0 && targetId.length > 0) &&
    (!needsText || arg.trim().length > 0);

  return (
    <div className="card space-y-3">
      <div className="font-medium text-zinc-200">New schedule</div>
      <div>
        <span className="text-[11px] uppercase tracking-wider font-semibold text-zinc-500">Action</span>
        <div className="flex flex-wrap gap-1 mt-1">
          {(['restart', 'command', 'broadcast', 'backup'] as const).map((k) => (
            <button
              key={k}
              type="button"
              className={`px-3 py-1.5 rounded-lg text-sm capitalize ${kind === k ? 'bg-zinc-700 text-white' : 'bg-zinc-800/60 text-zinc-400 hover:text-white'}`}
              onClick={() => setKind(k)}
            >
              {k}
            </button>
          ))}
        </div>
      </div>
      {needsText && (
        <label className="block">
          <span className="text-[11px] uppercase tracking-wider font-semibold text-zinc-500">
            {kind === 'command' ? 'Console command' : 'Message'}
          </span>
          <input className="input w-full mt-1" value={arg} onChange={(e) => setArg(e.target.value)}
            placeholder={kind === 'command' ? 'say Server restarting soon' : 'Restarting in 5 minutes'} />
        </label>
      )}
      {kind === 'backup' && (
        <div className="block">
          <span className="text-[11px] uppercase tracking-wider font-semibold text-zinc-500">Backup target</span>
          {targets === null ? (
            <div className="flex items-center gap-2 text-zinc-400 text-sm mt-1"><Loader2 size={14} className="animate-spin" /> Loading targets…</div>
          ) : targets.length === 0 ? (
            <p className="text-[13px] text-amber-300/90 mt-1">
              No S3 backup target configured. Add one in Settings → Backup Storage first.
            </p>
          ) : (
            <select className="input w-full mt-1" value={targetId} onChange={(e) => setTargetId(e.target.value)}>
              {targets.map((t) => (
                <option key={t.id} value={t.id}>{t.name}</option>
              ))}
            </select>
          )}
          <div className="grid grid-cols-2 gap-2 mt-2">
            <label className="block">
              <span className="text-[11px] uppercase tracking-wider font-semibold text-zinc-500">Keep last</span>
              <input type="number" min="0" className="input w-full mt-1" value={keepLast}
                onChange={(e) => setKeepLast(e.target.value)} placeholder="e.g. 7" />
            </label>
            <label className="block">
              <span className="text-[11px] uppercase tracking-wider font-semibold text-zinc-500">Delete older than (days)</span>
              <input type="number" min="0" className="input w-full mt-1" value={maxAgeDays}
                onChange={(e) => setMaxAgeDays(e.target.value)} placeholder="off" />
            </label>
          </div>
          <p className="text-[11px] text-zinc-500 mt-1">
            Archives the server's data to the selected S3 target (Minecraft Java is flushed first). Retention runs after
            each backup: the newest "keep last" are always kept; older ones beyond that are pruned (plus anything past the
            age limit, if set). Leave both blank to keep everything.
          </p>
        </div>
      )}
      <div>
        <span className="text-[11px] uppercase tracking-wider font-semibold text-zinc-500">When (cron)</span>
        <div className="flex flex-wrap gap-1 mt-1 mb-2">
          {CRON_PRESETS.map((p) => (
            <button key={p.cron} type="button"
              className={`px-2.5 py-1 rounded-md text-xs ${cron === p.cron ? 'bg-emerald-500/20 text-emerald-300' : 'bg-zinc-800/60 text-zinc-400 hover:text-white'}`}
              onClick={() => setCron(p.cron)}>
              {p.label}
            </button>
          ))}
        </div>
        <input className="input w-full font-mono" value={cron} onChange={(e) => setCron(e.target.value)} placeholder="0 5 * * *" />
        <p className="text-[11px] text-zinc-500 mt-1">5-field cron: minute hour day-of-month month day-of-week (host local time).</p>
      </div>
      <div className="flex items-center gap-2">
        <button className="btn btn-primary" onClick={submit} disabled={!valid || saving}>
          {saving ? <Loader2 size={15} className="animate-spin" /> : <Check size={15} />} Add schedule
        </button>
        <button className="btn btn-secondary" onClick={onCancel} disabled={saving}><X size={15} /> Cancel</button>
      </div>
    </div>
  );
}
