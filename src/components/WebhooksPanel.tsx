/**
 * Alerts — outbound webhooks (Discord / Slack / custom) the host POSTs to when
 * one of its servers crashes (or crash-loops). The full URL stays Rust-side on
 * this machine; the UI only ever sees a redacted hint, and the cloud never sees
 * it at all. Host-local: these fire for THIS desktop's own servers.
 */
import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Webhook, Plus, Trash2, Loader2, Check, X, Send, Power, PowerOff } from 'lucide-react';

type WebhookKind = 'discord' | 'slack' | 'generic';

interface WebhookView {
  id: string;
  name: string;
  kind: WebhookKind;
  urlHint: string;
  enabled: boolean;
}

const KIND_LABEL: Record<WebhookKind, string> = {
  discord: 'Discord',
  slack: 'Slack',
  generic: 'Custom (raw JSON)',
};

export function WebhooksPanel() {
  const [hooks, setHooks] = useState<WebhookView[] | null>(null);
  const [adding, setAdding] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [tested, setTested] = useState<Record<string, 'ok' | 'fail'>>({});

  const refresh = useCallback(async () => {
    setErr(null);
    try {
      setHooks(await invoke<WebhookView[]>('list_webhooks'));
    } catch (e) {
      setErr(String(e));
      setHooks([]);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  async function remove(id: string) {
    if (!confirm('Delete this alert webhook?')) return;
    setBusy(id);
    try {
      await invoke('remove_webhook', { id });
      await refresh();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function toggle(h: WebhookView) {
    setBusy(h.id);
    try {
      await invoke('set_webhook_enabled', { id: h.id, enabled: !h.enabled });
      await refresh();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function test(id: string) {
    setBusy(id);
    setTested((r) => {
      const next = { ...r };
      delete next[id];
      return next;
    });
    try {
      await invoke('test_webhook', { id });
      setTested((r) => ({ ...r, [id]: 'ok' }));
    } catch {
      setTested((r) => ({ ...r, [id]: 'fail' }));
    } finally {
      setBusy(null);
    }
  }

  return (
    <div className="space-y-4">
      <div className="card">
        <div className="flex items-center justify-between mb-1">
          <div className="flex items-center gap-2 font-medium text-zinc-200">
            <Webhook size={16} className="text-emerald-400" /> Crash alerts
          </div>
          {!adding && (
            <button className="btn btn-secondary btn-sm" onClick={() => setAdding(true)}>
              <Plus size={13} /> Add webhook
            </button>
          )}
        </div>
        <p className="text-xs text-zinc-500">
          Get pinged when a server crashes. The host posts directly to your webhook — the URL stays
          on this machine and never touches the cloud. Fires for this desktop's servers.
        </p>
      </div>

      {err && <div className="text-sm text-red-300 px-1 break-all">{err}</div>}

      {adding && <AddWebhookForm onSaved={() => { setAdding(false); void refresh(); }} onCancel={() => setAdding(false)} />}

      <div className="card">
        {hooks === null ? (
          <div className="flex items-center gap-2 text-zinc-400 py-3">
            <Loader2 size={16} className="animate-spin" /> Loading…
          </div>
        ) : hooks.length === 0 ? (
          <p className="text-sm text-zinc-500 py-3">
            No alert webhooks yet. Add one to get a Discord/Slack ping when a server crashes.
          </p>
        ) : (
          <div className="divide-y divide-zinc-800">
            {hooks.map((h) => (
              <div key={h.id} className="flex items-center gap-3 py-2.5">
                <div className="min-w-0 flex-1">
                  <div className="text-[13px] text-zinc-200 truncate">
                    {h.name} <span className="text-zinc-500">· {KIND_LABEL[h.kind]}</span>
                  </div>
                  <div className="text-xs text-zinc-500 font-mono truncate">{h.urlHint}</div>
                </div>
                {tested[h.id] === 'ok' && <span className="text-[11px] text-emerald-400 flex items-center gap-1"><Check size={12} /> Sent</span>}
                {tested[h.id] === 'fail' && <span className="text-[11px] text-red-400 flex items-center gap-1"><X size={12} /> Failed</span>}
                <button className="icon-btn text-zinc-400 hover:text-sky-400" title="Send a test alert" onClick={() => test(h.id)} disabled={busy === h.id}>
                  {busy === h.id ? <Loader2 size={14} className="animate-spin" /> : <Send size={14} />}
                </button>
                <button
                  className={`inline-flex items-center gap-1 shrink-0 px-2 py-0.5 rounded-full border text-[11px] font-medium transition-colors ${
                    h.enabled
                      ? 'border-emerald-500/40 bg-emerald-500/10 text-emerald-300 hover:bg-emerald-500/20'
                      : 'border-zinc-700 bg-zinc-800/60 text-zinc-400 hover:text-zinc-200'
                  }`}
                  title={h.enabled ? 'Alerts on — click to pause' : 'Alerts paused — click to enable'}
                  aria-pressed={h.enabled}
                  onClick={() => toggle(h)}
                  disabled={busy === h.id}
                >
                  {h.enabled ? <Power size={13} /> : <PowerOff size={13} />}
                  {h.enabled ? 'On' : 'Paused'}
                </button>
                <button className="icon-btn text-zinc-400 hover:text-red-400" title="Delete" onClick={() => remove(h.id)} disabled={busy === h.id}>
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

function AddWebhookForm({ onSaved, onCancel }: { onSaved: () => void; onCancel: () => void }) {
  const [name, setName] = useState('');
  const [kind, setKind] = useState<WebhookKind>('discord');
  const [url, setUrl] = useState('');
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const valid = name.trim().length > 0 && /^https:\/\/\S+$/.test(url.trim());

  async function save() {
    setSaving(true);
    setErr(null);
    try {
      await invoke('add_webhook', { name: name.trim(), kind, url: url.trim() });
      onSaved();
    } catch (e) {
      setErr(String(e));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="card space-y-3">
      <div className="font-medium text-zinc-200">New alert webhook</div>
      <label className="block">
        <span className="text-[11px] uppercase tracking-wider font-semibold text-zinc-500">Name</span>
        <input className="input w-full mt-1" value={name} onChange={(e) => setName(e.target.value)} placeholder="My Discord" />
      </label>
      <div>
        <span className="text-[11px] uppercase tracking-wider font-semibold text-zinc-500">Type</span>
        <div className="flex gap-1 mt-1">
          {(['discord', 'slack', 'generic'] as const).map((k) => (
            <button
              key={k}
              type="button"
              className={`px-3 py-1.5 rounded-lg text-sm ${kind === k ? 'bg-zinc-700 text-white' : 'bg-zinc-800/60 text-zinc-400 hover:text-white'}`}
              onClick={() => setKind(k)}
            >
              {KIND_LABEL[k]}
            </button>
          ))}
        </div>
      </div>
      <label className="block">
        <span className="text-[11px] uppercase tracking-wider font-semibold text-zinc-500">Webhook URL</span>
        <input
          className="input w-full mt-1 font-mono text-[12.5px]"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          placeholder={kind === 'discord' ? 'https://discord.com/api/webhooks/…' : 'https://…'}
          autoCapitalize="off"
          autoCorrect="off"
          spellCheck={false}
        />
      </label>
      {err && <div className="text-sm text-red-300 break-all">{err}</div>}
      <div className="flex items-center gap-2">
        <button className="btn btn-primary" onClick={save} disabled={!valid || saving}>
          {saving ? <Loader2 size={15} className="animate-spin" /> : <Check size={15} />} Add webhook
        </button>
        <button className="btn btn-secondary" onClick={onCancel} disabled={saving}>
          <X size={15} /> Cancel
        </button>
      </div>
    </div>
  );
}
