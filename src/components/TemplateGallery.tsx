/** Community template gallery: browse/import published Custom Games and publish your own. */
import { useCallback, useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import { invoke } from '@tauri-apps/api/core';
import { X, Loader2, Download, Store, UploadCloud, Trash2, LogIn } from 'lucide-react';
import { useEscapeClose } from '../hooks/useEscapeClose';
import { useAuthStore } from '../stores/authStore';
import { appConfirm } from '../stores/dialogStore';
import { describeError } from '../utils/errors';
import { LoginDialog } from './LoginDialog';

interface TemplateSummary {
  id: string;
  publisherName: string;
  name: string;
  gameLabel: string | null;
  description: string | null;
  downloads: number;
  createdAt: number;
  /** Published by the signed-in user. */
  mine: boolean;
}
interface TemplateList {
  templates: TemplateSummary[];
  nextBefore: number | null;
  nextBeforeId?: number | null;
}

export function TemplateGallery({
  customGames,
  onImported,
  onClose,
}: {
  customGames: Array<{ game_type: string; name: string }>;
  onImported: () => void;
  onClose: () => void;
}) {
  useEscapeClose(onClose);
  const me = useAuthStore((s) => s.me);
  const signedIn = !!me;
  const [loginOpen, setLoginOpen] = useState(false);
  const [list, setList] = useState<TemplateSummary[] | null>(null);
  const [cursor, setCursor] = useState<number | null>(null);
  // Rowid tiebreaker for the composite cursor.
  const [cursorId, setCursorId] = useState<number | null>(null);
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  // publish form
  const [pubGame, setPubGame] = useState<string>(customGames[0]?.game_type ?? '');
  const [pubName, setPubName] = useState('');
  const [pubDesc, setPubDesc] = useState('');
  const [publishing, setPublishing] = useState(false);

  const load = useCallback(async (before?: number, beforeId?: number) => {
    setLoading(true);
    setErr(null);
    try {
      const r = await invoke<TemplateList | null>('cloud_templates_list', {
        before: before ?? null,
        beforeId: beforeId ?? null,
        limit: 40,
      });
      // Validate outside the updater: a throw inside setState would unmount the whole app.
      const page = Array.isArray(r?.templates) ? r.templates : [];
      setList((cur) => (before && cur ? [...cur, ...page] : page));
      setCursor(r?.nextBefore ?? null);
      setCursorId(r?.nextBeforeId ?? null);
    } catch (e) {
      setErr(describeError(e));
      setList((cur) => cur ?? []);
    } finally {
      setLoading(false);
    }
  }, []);

  // The gallery is account-scoped (401 otherwise); load once signed in, and again after signing in here.
  useEffect(() => {
    if (signedIn) void load();
  }, [signedIn, load]);

  async function importTemplate(t: TemplateSummary) {
    setBusy(t.id);
    setErr(null);
    try {
      const json = await invoke<string>('cloud_template_get', { id: t.id });
      await invoke('import_game', { json });
      onImported();
      setNotice(`Imported “${t.name}” — find it in your games.`);
    } catch (e) {
      setErr(describeError(e));
    } finally {
      setBusy(null);
    }
  }

  async function unpublish(t: TemplateSummary) {
    const ok = await appConfirm({
      title: `Unpublish “${t.name}”?`,
      message: 'It disappears from the gallery; people who already imported it keep their copy.',
      confirmLabel: 'Unpublish',
      danger: true,
    });
    if (!ok) return;
    setBusy(t.id);
    setErr(null);
    try {
      const deleted = await invoke<boolean>('cloud_template_delete', { id: t.id });
      if (deleted) {
        setList((cur) => cur?.filter((x) => x.id !== t.id) ?? cur);
        setNotice(`Unpublished “${t.name}”.`);
      } else {
        setErr('That template is not yours or was already removed.');
      }
    } catch (e) {
      setErr(describeError(e));
    } finally {
      setBusy(null);
    }
  }

  async function publish() {
    const game = customGames.find((g) => g.game_type === pubGame);
    if (!game || !pubName.trim()) return;
    const ok = await appConfirm({
      title: 'Publish to the community gallery?',
      message:
        'This publishes the game definition PUBLICLY. Make sure no passwords or API keys are baked into its variable defaults.',
      confirmLabel: 'Publish',
    });
    if (!ok) return;
    setPublishing(true);
    setErr(null);
    try {
      const config = await invoke<string>('export_game', { gameType: pubGame });
      await invoke('cloud_template_publish', {
        name: pubName.trim(),
        gameLabel: game.name,
        description: pubDesc.trim() || null,
        config,
      });
      setNotice(`Published “${pubName.trim()}” to the gallery.`);
      setPubName('');
      setPubDesc('');
      void load();
    } catch (e) {
      setErr(describeError(e));
    } finally {
      setPublishing(false);
    }
  }

  return createPortal(
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm p-4" onClick={onClose} role="dialog" aria-modal="true">
      <div
        className="card card-elevated w-full max-w-2xl relative max-h-[85vh] overflow-auto"
        onClick={(e) => e.stopPropagation()}
      >
        <button onClick={onClose} className="absolute top-3 right-3 p-1 text-slate-400 hover:text-white rounded" title="Close" aria-label="Close">
          <X size={16} />
        </button>

        <div className="flex items-center gap-2 mb-1">
          <Store size={18} className="text-emerald-400" />
          <h2 className="text-lg font-bold text-slate-100">Community templates</h2>
        </div>
        <p className="text-[13px] text-slate-500 mb-4">
          Browse and import game-server setups others have shared — or publish your own.
        </p>

        {!signedIn ? (
          <div className="card text-center py-8">
            {me === undefined ? (
              <div className="flex items-center justify-center gap-2 text-zinc-400"><Loader2 size={16} className="animate-spin" /> Checking your account…</div>
            ) : (
              <>
                <p className="text-sm text-zinc-300">The gallery is tied to your LocalForge account.</p>
                <p className="text-xs text-zinc-500 mt-1 mb-4">Sign in (free) to browse, import and publish templates.</p>
                <button className="btn btn-primary btn-sm" onClick={() => setLoginOpen(true)}>
                  <LogIn size={14} /> Sign in / Create account
                </button>
                <LoginDialog open={loginOpen} onClose={() => setLoginOpen(false)} />
              </>
            )}
          </div>
        ) : (
          <>
            {notice && <div className="mb-3 text-[13px] text-emerald-300">{notice}</div>}
            {err && <div className="mb-3 text-[13px] text-red-300 break-all">{err}</div>}

            {/* Publish */}
            {customGames.length > 0 && (
              <div className="card mb-4">
                <div className="flex items-center gap-2 font-medium text-zinc-200 mb-2">
                  <UploadCloud size={15} className="text-sky-400" /> Publish a template
                </div>
                <div className="grid gap-2 sm:grid-cols-2">
                  <select className="input" value={pubGame} onChange={(e) => setPubGame(e.target.value)}>
                    {customGames.map((g) => (
                      <option key={g.game_type} value={g.game_type}>{g.name}</option>
                    ))}
                  </select>
                  <input className="input" placeholder="Template name" value={pubName} onChange={(e) => setPubName(e.target.value)} />
                </div>
                <input className="input w-full mt-2" placeholder="Short description (optional)" value={pubDesc} onChange={(e) => setPubDesc(e.target.value)} />
                <button className="btn btn-secondary btn-sm mt-2" onClick={publish} disabled={publishing || !pubName.trim()}>
                  {publishing ? <Loader2 size={14} className="animate-spin" /> : <UploadCloud size={14} />} Publish (public)
                </button>
              </div>
            )}

            {/* Browse */}
            {list === null ? (
              <div className="flex items-center gap-2 text-zinc-400 py-4"><Loader2 size={16} className="animate-spin" /> Loading…</div>
            ) : list.length === 0 ? (
              <p className="text-sm text-zinc-500 py-4">
                {err ? 'Couldn’t load the gallery right now.' : 'No community templates yet. Be the first to publish one!'}
              </p>
            ) : (
              <div className="divide-y divide-zinc-800">
                {list.map((t) => (
                  <div key={t.id} className="flex items-center gap-3 py-2.5">
                    <div className="min-w-0 flex-1">
                      <div className="text-[13.5px] text-slate-200 truncate">
                        {t.name}
                        {t.gameLabel && <span className="text-slate-500"> · {t.gameLabel}</span>}
                      </div>
                      <div className="text-xs text-slate-500 truncate">
                        by {t.mine ? 'you' : t.publisherName} · {t.downloads} import{t.downloads === 1 ? '' : 's'}
                        {t.description ? ` · ${t.description}` : ''}
                      </div>
                    </div>
                    {t.mine && (
                      <button
                        className="btn btn-secondary btn-sm shrink-0 text-red-300"
                        onClick={() => unpublish(t)}
                        disabled={busy === t.id}
                        title="Remove this template from the gallery"
                      >
                        <Trash2 size={14} /> Unpublish
                      </button>
                    )}
                    <button className="btn btn-secondary btn-sm shrink-0" onClick={() => importTemplate(t)} disabled={busy === t.id}>
                      {busy === t.id ? <Loader2 size={14} className="animate-spin" /> : <Download size={14} />} Import
                    </button>
                  </div>
                ))}
              </div>
            )}
            {cursor != null && (
              <button className="btn btn-secondary btn-sm mt-3" onClick={() => load(cursor, cursorId ?? undefined)} disabled={loading}>
                {loading ? <Loader2 size={14} className="animate-spin" /> : null} Load more
              </button>
            )}
            {err && list !== null && list.length > 0 && (
              <button className="btn btn-secondary btn-sm mt-3 ml-2" onClick={() => load()} disabled={loading}>Retry</button>
            )}
          </>
        )}
      </div>
    </div>,
    document.body,
  );
}
