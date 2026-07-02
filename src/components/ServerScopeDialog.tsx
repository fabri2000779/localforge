/**
 * Per-server access editor for a Team member. Lets an admin narrow a member's
 * access from "every server in the org" (their role's default) down to a
 * specific allowlist — optionally as a temporary 48h grant. The cloud stores a
 * plaintext server-id allowlist and the relay enforces it on server.* commands.
 */
import { useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import { invoke } from '@tauri-apps/api/core';
import { X, Loader2, Shield } from 'lucide-react';
import { useServerStore } from '../stores/serverStore';
import { useEscapeClose } from '../hooks/useEscapeClose';

interface MemberScope {
  serverId: string;
  expiresAt: number | null;
}

export function ServerScopeDialog({
  orgId,
  member,
  onClose,
}: {
  orgId: string;
  member: { id: string; name: string };
  onClose: () => void;
}) {
  const servers = useServerStore((s) => s.servers);
  const [loading, setLoading] = useState(true);
  const [restricted, setRestricted] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [temporary, setTemporary] = useState(false);
  // The deadline as LOADED — preserved on save so re-saving doesn't silently
  // re-extend the grant to +48h every time (audit finding). Cleared when the
  // admin unchecks "temporary", so re-checking mints a fresh 48h window.
  const [loadedExpiresAt, setLoadedExpiresAt] = useState<number | null>(null);
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  useEscapeClose(onClose, !saving);

  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        const scopes = await invoke<MemberScope[]>('cloud_member_scopes_get', {
          orgId,
          userId: member.id,
        });
        if (!alive) return;
        if (scopes.length > 0) {
          setRestricted(true);
          setSelected(new Set(scopes.map((s) => s.serverId)));
          const exp = scopes.find((s) => s.expiresAt != null)?.expiresAt ?? null;
          setTemporary(exp != null);
          setLoadedExpiresAt(exp);
        }
      } catch (e) {
        if (alive) setErr(String(e));
      } finally {
        if (alive) setLoading(false);
      }
    })();
    return () => {
      alive = false;
    };
  }, [orgId, member.id]);

  function toggle(id: string) {
    setSelected((cur) => {
      const n = new Set(cur);
      if (n.has(id)) n.delete(id);
      else n.add(id);
      return n;
    });
  }

  async function save() {
    setSaving(true);
    setErr(null);
    try {
      // Preserve the existing deadline; only mint +48h for a NEW grant.
      const expiresAt = temporary ? (loadedExpiresAt ?? Date.now() + 48 * 3600 * 1000) : null;
      const scopes = restricted ? [...selected].map((serverId) => ({ serverId, expiresAt })) : [];
      await invoke('cloud_member_scopes_set', { orgId, userId: member.id, scopes });
      onClose();
    } catch (e) {
      setErr(String(e));
      setSaving(false);
    }
  }

  return createPortal(
    <div className="auth-overlay" onClick={onClose} role="dialog" aria-modal="true">
      <div className="auth-modal" onClick={(e) => e.stopPropagation()} style={{ maxWidth: 460 }}>
        <button className="auth-close" onClick={onClose}>
          <X size={16} strokeWidth={2.2} />
        </button>
        <div className="auth-header">
          <h2>
            <Shield size={16} style={{ display: 'inline', marginRight: 6, verticalAlign: '-2px' }} />
            Server access
          </h2>
          <p>
            Limit which servers <strong>{member.name}</strong> can control. Their org role still
            applies — this just narrows it to specific servers.
          </p>
        </div>

        {loading ? (
          <div className="flex items-center gap-2 text-zinc-400 py-3">
            <Loader2 size={16} className="animate-spin" /> Loading…
          </div>
        ) : (
          <>
            <div className="space-y-2">
              <label className="flex items-center gap-2 text-sm text-zinc-200">
                <input type="radio" checked={!restricted} onChange={() => setRestricted(false)} /> Full
                access (all servers)
              </label>
              <label className="flex items-center gap-2 text-sm text-zinc-200">
                <input type="radio" checked={restricted} onChange={() => setRestricted(true)} /> Only
                specific servers
              </label>
            </div>

            {restricted && (
              <div className="mt-3 max-h-56 overflow-auto border border-zinc-800 rounded-lg p-2 space-y-1">
                {servers.length === 0 ? (
                  <p className="text-xs text-zinc-500 p-1">No servers synced yet.</p>
                ) : (
                  servers.map((s) => (
                    <label
                      key={s.id}
                      className="flex items-center gap-2 text-[13px] text-zinc-300 px-1 py-0.5"
                    >
                      <input type="checkbox" checked={selected.has(s.id)} onChange={() => toggle(s.id)} />
                      {s.name}
                    </label>
                  ))
                )}
              </div>
            )}

            {restricted && (
              <>
                <label className="flex items-center gap-2 text-[13px] text-zinc-400 mt-3">
                  <input
                    type="checkbox"
                    checked={temporary}
                    onChange={(e) => {
                      setTemporary(e.target.checked);
                      // Unchecking abandons the old deadline — re-checking
                      // grants a fresh 48h window.
                      if (!e.target.checked) setLoadedExpiresAt(null);
                    }}
                  />
                  Temporary — access expires in 48 hours
                </label>
                {temporary && loadedExpiresAt != null && (
                  <p className="text-[12px] text-zinc-500 mt-1 ml-6">
                    Expires {new Date(loadedExpiresAt).toLocaleString()} (in{' '}
                    {Math.max(0, Math.round((loadedExpiresAt - Date.now()) / 3_600_000))}h). Saving
                    keeps this deadline.
                  </p>
                )}
              </>
            )}

            {err && <div className="auth-err mt-2 break-all">{err}</div>}

            <div style={{ display: 'flex', gap: 8, marginTop: 14 }}>
              <button
                className="auth-submit"
                style={{ flex: 1 }}
                onClick={save}
                disabled={saving || (restricted && selected.size === 0)}
              >
                {saving ? 'Saving…' : 'Save access'}
              </button>
              <button className="btn btn-secondary" onClick={onClose} disabled={saving}>
                Cancel
              </button>
            </div>
          </>
        )}
      </div>
    </div>,
    document.body,
  );
}
