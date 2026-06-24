/**
 * First-run onboarding nudge.
 *
 * Shows once, only for a genuinely fresh install: Docker is up and there are no
 * servers yet. It points the user at creating their first server and hints at
 * the optional cloud features (phone control + crash alerts). Dismissal is
 * sticky (localStorage) so it never nags again. No account or network needed —
 * this is pure orchestration over what's already on screen.
 */
import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Rocket, Server, Smartphone, X, Check } from 'lucide-react';
import { useDockerStore } from '../stores/dockerStore';
import { useServerStore } from '../stores/serverStore';

const SEEN_KEY = 'lf_onboarded_v1';

export function OnboardingWizard() {
  const status = useDockerStore((s) => s.status);
  const servers = useServerStore((s) => s.servers);
  const navigate = useNavigate();
  const [dismissed, setDismissed] = useState(
    () => typeof localStorage !== 'undefined' && !!localStorage.getItem(SEEN_KEY),
  );

  // Only for a fresh start: Docker ready and no servers yet.
  const fresh = !!status?.running && servers.length === 0;
  if (dismissed || !fresh) return null;

  const close = () => {
    try {
      localStorage.setItem(SEEN_KEY, String(Date.now()));
    } catch {
      /* private mode / storage blocked — worst case it shows once more */
    }
    setDismissed(true);
  };
  const createServer = () => {
    close();
    navigate('/games');
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm p-4"
      onClick={close}
    >
      <div className="card card-elevated w-full max-w-md relative" onClick={(e) => e.stopPropagation()}>
        <button
          onClick={close}
          className="absolute top-3 right-3 p-1 text-slate-400 hover:text-white rounded transition-colors"
          title="Close"
        >
          <X size={16} />
        </button>

        <div className="flex items-center gap-3 mb-3">
          <div className="w-11 h-11 rounded-xl bg-emerald-500/15 flex items-center justify-center shrink-0">
            <Rocket size={22} className="text-emerald-400" />
          </div>
          <div>
            <div className="eyebrow mb-0.5">Welcome to LocalForge</div>
            <h2 className="text-lg font-bold text-slate-100">You're all set up</h2>
          </div>
        </div>

        <div className="flex items-center gap-2 text-[13px] text-slate-300 mb-4 px-3 py-2 rounded-lg bg-[var(--color-surface)] border border-[var(--color-border)]">
          <Check size={15} className="text-emerald-400 shrink-0" />
          Docker detected — your machine is ready to host game servers.
        </div>

        <p className="text-[13.5px] text-slate-400 mb-4 leading-relaxed">
          LocalForge runs game servers right on your own machine. Pick a game and you'll have a
          server running in a click — no account needed.
        </p>

        <button className="btn btn-primary w-full justify-center" onClick={createServer}>
          <Server size={16} /> Create my first server
        </button>

        <div className="flex items-start gap-2 mt-4 text-[12px] text-slate-500 leading-relaxed">
          <Smartphone size={14} className="text-slate-400 shrink-0 mt-0.5" />
          <span>
            Optional: sign in under <strong className="text-slate-400">Settings → Account</strong> to
            control your servers from your phone and get crash alerts.
          </span>
        </div>

        <button
          className="text-[12.5px] text-slate-500 hover:text-slate-300 mt-3 mx-auto block transition-colors"
          onClick={close}
        >
          I'll explore on my own
        </button>
      </div>
    </div>
  );
}
