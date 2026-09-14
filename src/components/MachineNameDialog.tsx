/** First-run "name this machine" prompt, shown once per install (gated by `this_machine.toml`, not
 *  localStorage, which WebView resets wipe). Waits for the local node to exist. */
import { useEffect, useRef, useState } from 'react';
import { Server, Check } from 'lucide-react';
import { useNodesStore } from '../stores/nodesStore';

const LEGACY_SEEN_KEY = 'lf.onboard.machine.v1';

export function MachineNameDialog() {
  const thisMachine = useNodesStore((s) => s.thisMachine);
  const fetchThisMachine = useNodesStore((s) => s.fetchThisMachine);
  const renameMachine = useNodesStore((s) => s.renameMachine);
  const dismissPrompt = useNodesStore((s) => s.dismissMachineNamePrompt);

  const [open, setOpen] = useState(false);
  const [name, setName] = useState('');
  const [saving, setSaving] = useState(false);
  // Guards the one-time legacy migration against loops while the store refreshes.
  const migratedRef = useRef(false);

  // Covers the signed-out local-only path (the bridge also fetches it).
  useEffect(() => {
    if (!thisMachine) void fetchThisMachine();
  }, [thisMachine, fetchThisMachine]);

  // One-time migration of the old localStorage dismissal flag into the toml.
  useEffect(() => {
    if (migratedRef.current) return;
    if (!thisMachine) return;
    if (thisMachine.name_prompt_dismissed_at) {
      migratedRef.current = true;
      return;
    }
    if (typeof localStorage === 'undefined') return;
    if (!localStorage.getItem(LEGACY_SEEN_KEY)) return;
    migratedRef.current = true;
    void dismissPrompt().catch(() => { /* best-effort — non-fatal */ });
  }, [thisMachine, dismissPrompt]);

  // Open once the identity is known and the prompt was never dismissed.
  useEffect(() => {
    if (open) return;
    if (!thisMachine) return;
    if (thisMachine.name_prompt_dismissed_at) return;
    // Don't flash the dialog while the legacy migration is about to run.
    if (
      typeof localStorage !== 'undefined' &&
      localStorage.getItem(LEGACY_SEEN_KEY)
    ) {
      return;
    }
    setName(thisMachine.name);
    setOpen(true);
  }, [thisMachine, open]);

  function dismiss() {
    setOpen(false);
    // Persist in the toml; best-effort (worst case we ask again next launch).
    void dismissPrompt().catch(() => {});
  }

  async function save() {
    const trimmed = name.trim();
    setSaving(true);
    try {
      if (trimmed && trimmed !== thisMachine?.name) {
        await renameMachine(trimmed);
      }
    } catch {
      /* keep the default name on failure — non-fatal */
    } finally {
      setSaving(false);
      dismiss();
    }
  }

  if (!open) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/70 backdrop-blur-sm" onClick={dismiss} />
      <div className="relative bg-zinc-900 border border-orange-500/40 rounded-xl shadow-2xl max-w-md w-full mx-4 p-6 animate-fade-in">
        <div className="flex items-center gap-3 mb-4">
          <div className="w-12 h-12 rounded-lg bg-orange-500/15 flex items-center justify-center shrink-0">
            <Server size={24} className="text-orange-400" />
          </div>
          <div>
            <h3 className="text-lg font-semibold text-slate-100">Name this machine</h3>
            <p className="text-sm text-slate-400">So you can tell your devices apart</p>
          </div>
        </div>

        <p className="text-[13px] text-slate-400 leading-relaxed mb-4">
          When you sign in and run servers across more than one computer or
          agent, this name labels <strong className="text-slate-200">this</strong>{' '}
          device in your fleet. You can change it anytime from the Nodes page.
        </p>

        <label className="block text-[11px] uppercase tracking-wider font-semibold text-slate-500 mb-1.5">
          Machine name
        </label>
        <input
          type="text"
          value={name}
          onChange={(e) => setName(e.target.value)}
          autoFocus
          maxLength={48}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && name.trim()) void save();
          }}
          className="input w-full mb-5"
          placeholder="e.g. Home PC, Office desktop"
        />

        <div className="flex items-center gap-3">
          <button
            onClick={save}
            disabled={saving || !name.trim()}
            className="btn btn-primary flex-1"
          >
            <Check size={16} /> {saving ? 'Saving…' : 'Save name'}
          </button>
          <button onClick={dismiss} disabled={saving} className="btn btn-secondary">
            Skip
          </button>
        </div>
      </div>
    </div>
  );
}
