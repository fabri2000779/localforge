/**
 * Players tab — live roster + quick moderation for the active server.
 *
 * Reads/acts through the active node's backend (`list_players` / `player_action`)
 * — local Docker or an agent over HTTPS; the cloud is never involved. Only shown
 * for games we adapt (currently Minecraft Java, via the server console).
 *
 * Listing is best-effort: the host sends `list` and reads the response back from
 * the log, so the roster reflects the last sample (auto-refreshes every 30s).
 */
import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Users, Loader2, RotateCcw, UserX, Ban, ShieldCheck, ShieldOff, Gavel } from 'lucide-react';

interface Player {
  name: string;
  id?: string;
}
type Kind = 'kick' | 'ban' | 'unban' | 'op' | 'deop';

const BTN = 'inline-flex items-center gap-1 px-2 py-1 rounded-md text-xs bg-zinc-800/60 hover:bg-zinc-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors';
const BTN2 = 'inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-md text-xs bg-zinc-800 border border-zinc-700 hover:bg-zinc-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors';

export function PlayersPanel({ serverId, nodeId }: { serverId: string; nodeId: string }) {
  const [players, setPlayers] = useState<Player[]>([]);
  const [loading, setLoading] = useState(true);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null); // `${name}:${kind}`
  const [name, setName] = useState('');
  const firstLoad = useRef(true);

  const refresh = useCallback(async () => {
    setErr(null);
    try {
      setPlayers(await invoke<Player[]>('list_players', { serverId, nodeId }));
    } catch (e) {
      setErr(String(e));
    } finally {
      setLoading(false);
      firstLoad.current = false;
    }
  }, [serverId, nodeId]);

  useEffect(() => {
    setLoading(true);
    void refresh();
    // Listing sends `list` to the console (which echoes into the log), so keep
    // the cadence gentle to avoid spamming the Console tab. A future RCON
    // adapter would poll out-of-band and could refresh more often.
    const t = setInterval(() => void refresh(), 30_000);
    return () => clearInterval(t);
  }, [refresh]);

  const act = async (kind: Kind, target: string, askReason = false) => {
    const who = target.trim();
    if (!who) return;
    let reason: string | undefined;
    if (askReason) {
      const r = window.prompt(`Reason for ${kind} on "${who}" (optional):`);
      if (r === null) return; // cancelled
      reason = r.trim() || undefined;
    }
    setBusy(`${who}:${kind}`);
    setErr(null);
    try {
      await invoke('player_action', {
        serverId,
        nodeId,
        action: { kind, name: who, ...(reason ? { reason } : {}) },
      });
      // Let the server apply it, then refresh the roster.
      window.setTimeout(() => void refresh(), 600);
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(null);
    }
  };

  const isBusy = (n: string, k: Kind) => busy === `${n}:${k}`;

  return (
    <div className="space-y-4">
      <div className="card">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2 font-medium text-zinc-200">
            <Users size={16} className="text-emerald-400" /> Online players
            <span className="text-zinc-500 font-normal">({players.length})</span>
          </div>
          <button className={`${BTN} text-zinc-400`} title="Refresh" onClick={() => void refresh()} disabled={loading}>
            <RotateCcw size={14} className={loading ? 'animate-spin' : ''} /> Refresh
          </button>
        </div>
        <p className="text-xs text-zinc-500 mt-1">Roster comes from the server console and refreshes every 30s.</p>
      </div>

      {err && <div className="text-sm text-red-300 px-1 break-all">{err}</div>}

      {loading && firstLoad.current ? (
        <div className="card flex items-center gap-2 text-zinc-400 py-8 justify-center"><Loader2 size={16} className="animate-spin" /> Loading…</div>
      ) : players.length === 0 ? (
        <div className="card text-center text-zinc-500 py-8 text-sm">
          No players online. The server must be running to see its roster.
        </div>
      ) : (
        <div className="card p-0 overflow-hidden divide-y divide-zinc-800/70">
          {players.map((p) => (
            <div key={p.name} className="flex items-center justify-between px-4 py-2.5">
              <span className="flex items-center gap-2 text-sm text-zinc-200">
                <span className="w-2 h-2 rounded-full bg-emerald-400" /> {p.name}
              </span>
              <div className="flex items-center gap-1">
                <button className={`${BTN} text-zinc-300`} title="Make operator" disabled={isBusy(p.name, 'op')} onClick={() => act('op', p.name)}>
                  <ShieldCheck size={14} /> Op
                </button>
                <button className={`${BTN} text-amber-300`} title="Kick" disabled={isBusy(p.name, 'kick')} onClick={() => act('kick', p.name, true)}>
                  <UserX size={14} /> Kick
                </button>
                <button className={`${BTN} text-red-300`} title="Ban" disabled={isBusy(p.name, 'ban')} onClick={() => act('ban', p.name, true)}>
                  <Ban size={14} /> Ban
                </button>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* Moderate by name — for offline players (ban/pardon/op/deop). */}
      <div className="card space-y-2">
        <div className="flex items-center gap-2 text-sm text-zinc-300"><Gavel size={14} className="text-zinc-400" /> Moderate by name</div>
        <p className="text-xs text-zinc-500">Act on someone who isn't online — e.g. ban or pardon a name directly.</p>
        <input
          className="input"
          placeholder="Player name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Enter' && name.trim()) act('ban', name, true); }}
        />
        <div className="flex flex-wrap items-center gap-2">
          <button className={`${BTN2} text-red-300`} disabled={!name.trim() || isBusy(name.trim(), 'ban')} onClick={() => act('ban', name, true)}>
            <Ban size={14} /> Ban
          </button>
          <button className={`${BTN2} text-emerald-300`} disabled={!name.trim() || isBusy(name.trim(), 'unban')} onClick={() => act('unban', name)}>
            <ShieldOff size={14} /> Pardon
          </button>
          <button className={`${BTN2} text-zinc-200`} disabled={!name.trim() || isBusy(name.trim(), 'op')} onClick={() => act('op', name)}>
            <ShieldCheck size={14} /> Op
          </button>
          <button className={`${BTN2} text-zinc-200`} disabled={!name.trim() || isBusy(name.trim(), 'deop')} onClick={() => act('deop', name)}>
            <ShieldOff size={14} /> De-op
          </button>
        </div>
      </div>
    </div>
  );
}
