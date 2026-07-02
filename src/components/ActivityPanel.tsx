/**
 * Team activity feed — surfaces the org's audit log ("who did what, where,
 * when"). Reads GET /v1/audit via the cloud_audit_list command. Team-plan +
 * admin only (the cloud enforces; a 402 here shows the upgrade hint). The feed
 * is metadata only — no server config or secrets — so it's E2E-safe.
 */
import { useCallback, useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Activity, Loader2, RefreshCw, Lock } from 'lucide-react';
import { useServerStore } from '../stores/serverStore';

interface AuditEntry {
  actorUserId: string | null;
  actorName: string;
  action: string;
  target: string | null;
  metadata: Record<string, unknown> | null;
  createdAt: number;
}
interface AuditList {
  entries: AuditEntry[];
  nextBefore: number | null;
  /** Rowid tiebreaker — with `nextBefore` it forms the composite cursor. */
  nextBeforeId: number | null;
}

const ACTION_LABEL: Record<string, string> = {
  'server.start': 'started',
  'server.stop': 'stopped',
  'server.restart': 'restarted',
  'server.delete': 'deleted',
  'server.send_command': 'ran a command on',
  'server.update_config': 'edited the config of',
};

function describe(e: AuditEntry, serverName?: string): string {
  const verb = ACTION_LABEL[e.action] ?? e.action;
  if (!e.target) return verb;
  // Resolve the target to a server NAME when we know it — raw UUIDs made the
  // feed unreadable (audit finding). Unknown ids (deleted servers, other
  // nodes) fall back to a short id; the full id lives in the hover title.
  return `${verb} ${serverName ?? `${e.target.slice(0, 8)}…`}`;
}

function ago(ts: number): string {
  const s = Math.max(0, Math.floor((Date.now() - ts) / 1000));
  if (s < 60) return 'just now';
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ago`;
  const d = Math.floor(h / 24);
  return `${d}d ago`;
}

export function ActivityPanel() {
  // Local server inventory for target-id → name resolution in the feed.
  const servers = useServerStore((s) => s.servers);
  const serverNames = useMemo(
    () => new Map(servers.map((s) => [s.id, s.name] as const)),
    [servers],
  );
  const [entries, setEntries] = useState<AuditEntry[] | null>(null);
  const [cursor, setCursor] = useState<{ before: number; beforeId: number | null } | null>(null);
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [needsTeam, setNeedsTeam] = useState(false);

  const load = useCallback(async (before?: number, beforeId?: number | null) => {
    setLoading(true);
    setErr(null);
    try {
      const res = await invoke<AuditList>('cloud_audit_list', {
        before: before ?? null,
        beforeId: beforeId ?? null,
        limit: 50,
      });
      setEntries((cur) => (before && cur ? [...cur, ...res.entries] : res.entries));
      setCursor(
        res.nextBefore != null
          ? { before: res.nextBefore, beforeId: res.nextBeforeId ?? null }
          : null,
      );
    } catch (e) {
      const s = String(e);
      if (s.includes('plan_required') || s.includes('402')) setNeedsTeam(true);
      else setErr(s);
      // Only wipe on a FIRST-page failure — a failed "Load older" keeps the
      // already-loaded feed + cursor so the user can just retry (audit
      // finding: pagination errors blanked the whole panel).
      if (!before) setEntries([]);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  if (needsTeam) {
    return (
      <div className="card text-center py-10">
        <Lock size={26} className="text-zinc-500 mx-auto mb-3" />
        <div className="font-medium text-zinc-200 mb-1">Activity feed is a Team feature</div>
        <p className="text-sm text-zinc-500">
          Upgrade to Team to see who started, stopped, or changed servers across your org.
        </p>
      </div>
    );
  }

  return (
    <div className="space-y-4">
      <div className="card">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2 font-medium text-zinc-200">
            <Activity size={16} className="text-emerald-400" /> Activity
          </div>
          <button className="icon-btn text-zinc-400 hover:text-white" title="Refresh" onClick={() => load()} disabled={loading}>
            <RefreshCw size={15} className={loading && entries === null ? 'animate-spin' : ''} />
          </button>
        </div>
        <p className="text-xs text-zinc-500 mt-1">
          Who did what across your org. Kept for 30 days.
        </p>
      </div>

      {err && <div className="text-sm text-red-300 px-1 break-all">{err}</div>}

      <div className="card">
        {entries === null ? (
          <div className="flex items-center gap-2 text-zinc-400 py-3">
            <Loader2 size={16} className="animate-spin" /> Loading…
          </div>
        ) : entries.length === 0 ? (
          <p className="text-sm text-zinc-500 py-3">No activity recorded yet.</p>
        ) : (
          <>
            <div className="divide-y divide-zinc-800">
              {entries.map((e, i) => (
                <div key={`${e.createdAt}-${i}`} className="flex items-baseline gap-2 py-2.5 text-[13px]">
                  <span className="text-zinc-200 font-medium shrink-0">{e.actorName}</span>
                  <span className="text-zinc-400 min-w-0 flex-1 truncate" title={e.target ?? undefined}>
                    {describe(e, e.target ? serverNames.get(e.target) : undefined)}
                  </span>
                  <span className="text-xs text-zinc-600 shrink-0" title={new Date(e.createdAt).toLocaleString()}>
                    {ago(e.createdAt)}
                  </span>
                </div>
              ))}
            </div>
            {cursor != null && (
              <button
                className="btn btn-secondary btn-sm mt-3"
                onClick={() => load(cursor.before, cursor.beforeId)}
                disabled={loading}
              >
                {loading ? <Loader2 size={14} className="animate-spin" /> : null} Load older
              </button>
            )}
          </>
        )}
      </div>
    </div>
  );
}
