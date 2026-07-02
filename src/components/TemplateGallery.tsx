/**
 * Community template gallery — browse + import published Custom Game
 * definitions, and publish your own. A "template" is the JSON `export_game`
 * produces; importing one runs `import_game` locally. Publishing shows a clear
 * warning first (the definition becomes public, so no secrets in defaults).
 */
import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { X, Loader2, Download, Store, UploadCloud } from 'lucide-react';
import { useEscapeClose } from '../hooks/useEscapeClose';

interface TemplateSummary {
  id: string;
  publisherName: string;
  name: string;
  gameLabel: string | null;
  description: string | null;
  downloads: number;
  createdAt: number;
}
interface TemplateList {
  templates: TemplateSummary[];
  nextBefore: number | null;
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
  const [list, setList] = useState<TemplateSummary[] | null>(null);
  const [cursor, setCursor] = useState<number | null>(null);
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  // publish form
  const [pubGame, setPubGame] = useState<string>(customGames[0]?.game_type ?? '');
  const [pubName, setPubName] = useState('');
  const [pubDesc, setPubDesc] = useState('');
  const [publishing, setPublishing] = useState(false);

  const load = useCallback(async (before?: number) => {
    setLoading(true);
    setErr(null);
    try {
      const r = await invoke<TemplateList>('cloud_templates_list', { before: before ?? null, limit: 40 });
      setList((cur) => (before && cur ? [...cur, ...r.templates] : r.templates));
      setCursor(r.nextBefore);
    } catch (e) {
      setErr(String(e));
      setList([]);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function importTemplate(t: TemplateSummary) {
    setBusy(t.id);
    setErr(null);
    try {
      const json = await invoke<string>('cloud_template_get', { id: t.id });
      await invoke('import_game', { json });
      onImported();
      setNotice(`Imported “${t.name}” — find it in your games.`);
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function publish() {
    const game = customGames.find((g) => g.game_type === pubGame);
    if (!game || !pubName.trim()) return;
    if (
      !window.confirm(
        'This publishes the game definition PUBLICLY. Make sure no passwords or API keys are baked into its variable defaults. Continue?',
      )
    )
      return;
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
      setErr(String(e));
    } finally {
      setPublishing(false);
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm p-4" onClick={onClose}>
      <div
        className="card card-elevated w-full max-w-2xl relative max-h-[85vh] overflow-auto"
        onClick={(e) => e.stopPropagation()}
      >
        <button onClick={onClose} className="absolute top-3 right-3 p-1 text-slate-400 hover:text-white rounded" title="Close">
          <X size={16} />
        </button>

        <div className="flex items-center gap-2 mb-1">
          <Store size={18} className="text-emerald-400" />
          <h2 className="text-lg font-bold text-slate-100">Community templates</h2>
        </div>
        <p className="text-[13px] text-slate-500 mb-4">
          Browse and import game-server setups others have shared — or publish your own.
        </p>

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
          <p className="text-sm text-zinc-500 py-4">No community templates yet. Be the first to publish one!</p>
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
                    by {t.publisherName} · {t.downloads} import{t.downloads === 1 ? '' : 's'}
                    {t.description ? ` · ${t.description}` : ''}
                  </div>
                </div>
                <button className="btn btn-secondary btn-sm shrink-0" onClick={() => importTemplate(t)} disabled={busy === t.id}>
                  {busy === t.id ? <Loader2 size={14} className="animate-spin" /> : <Download size={14} />} Import
                </button>
              </div>
            ))}
          </div>
        )}
        {cursor != null && (
          <button className="btn btn-secondary btn-sm mt-3" onClick={() => load(cursor)} disabled={loading}>
            {loading ? <Loader2 size={14} className="animate-spin" /> : null} Load more
          </button>
        )}
      </div>
    </div>
  );
}
