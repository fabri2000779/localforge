import { useEffect, useState } from 'react';
import { check, type Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import { Download, X, Loader2, CheckCircle2, AlertCircle } from 'lucide-react';

type Phase = 'idle' | 'downloading' | 'installing' | 'done' | 'error';

/**
 * Auto-update prompt. Checks the configured `latest.json` endpoint on
 * startup (and once an hour after) — if a newer version is available
 * it shows a top-right card with release notes and an Install button.
 *
 * Restarting the desktop app to apply the update does **not** affect
 * any running game-server containers — those are owned by Docker /
 * the remote agent, not by this process.
 */
export function UpdateChecker() {
  const [update, setUpdate] = useState<Update | null>(null);
  const [dismissed, setDismissed] = useState(false);
  const [phase, setPhase] = useState<Phase>('idle');
  const [progress, setProgress] = useState<{ done: number; total?: number }>({
    done: 0,
  });
  const [errorMsg, setErrorMsg] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;

    const probe = async () => {
      try {
        const result = await check();
        if (!cancelled && result?.available) {
          setUpdate(result);
        }
      } catch (e) {
        // Common in dev (no signed bundle) — log and stay silent.
        console.debug('[Updater] check failed:', e);
      }
    };

    // First check on mount, then every hour.
    probe();
    const interval = setInterval(probe, 60 * 60 * 1000);

    return () => {
      cancelled = true;
      clearInterval(interval);
    };
  }, []);

  if (!update || dismissed) return null;

  const handleInstall = async () => {
    setPhase('downloading');
    setErrorMsg(null);
    try {
      let totalBytes: number | undefined;
      let received = 0;
      await update.downloadAndInstall((event) => {
        if (event.event === 'Started') {
          totalBytes = event.data.contentLength ?? undefined;
          setProgress({ done: 0, total: totalBytes });
        } else if (event.event === 'Progress') {
          received += event.data.chunkLength;
          setProgress({ done: received, total: totalBytes });
        } else if (event.event === 'Finished') {
          setPhase('installing');
        }
      });
      setPhase('done');
      // Give the user 2s to see the success, then relaunch.
      setTimeout(() => {
        relaunch().catch(console.error);
      }, 2000);
    } catch (e) {
      console.error('[Updater] install failed:', e);
      setErrorMsg(String(e));
      setPhase('error');
    }
  };

  const pct =
    progress.total && progress.total > 0
      ? Math.min(100, (progress.done / progress.total) * 100)
      : null;

  return (
    <div className="fixed top-12 right-4 z-50 max-w-sm animate-fade-in">
      <div
        className="rounded-xl shadow-2xl p-4 backdrop-blur-md"
        style={{
          background:
            'linear-gradient(180deg, rgba(20, 26, 38, 0.95) 0%, rgba(14, 19, 28, 0.95) 100%)',
          border: '1px solid rgba(59, 130, 246, 0.28)',
          boxShadow:
            '0 0 0 1px rgba(59, 130, 246, 0.05), 0 24px 50px -20px rgba(0, 0, 0, 0.7)',
        }}
      >
        <div className="flex items-start gap-3 mb-3">
          <div
            className="w-9 h-9 rounded-lg shrink-0 flex items-center justify-center"
            style={{
              background:
                'linear-gradient(135deg, rgba(59,130,246,0.18), rgba(139,92,246,0.18))',
              boxShadow: '0 0 0 1px rgba(99, 102, 241, 0.18)',
            }}
          >
            <Download size={15} className="text-blue-300" />
          </div>
          <div className="flex-1 min-w-0">
            <div className="font-semibold text-[13px] text-slate-100 tracking-tight">
              Update available — v{update.version}
            </div>
            <div className="text-[11.5px] text-slate-500 mt-0.5">
              You&apos;re on v{update.currentVersion}
            </div>
          </div>
          {phase === 'idle' && (
            <button
              onClick={() => setDismissed(true)}
              className="text-slate-500 hover:text-white shrink-0 -m-1 p-1 rounded transition-colors hover:bg-slate-800"
              aria-label="Dismiss"
            >
              <X size={14} />
            </button>
          )}
        </div>

        {update.body && phase === 'idle' && (
          <p className="text-xs text-slate-400 mb-3 line-clamp-4 whitespace-pre-line">
            {update.body}
          </p>
        )}

        {phase === 'idle' && (
          <>
            <div className="text-[11px] text-slate-500 mb-3">
              Restarting won't affect your running game servers — only the
              desktop app reloads.
            </div>
            <div className="flex gap-2">
              <button
                onClick={() => setDismissed(true)}
                className="flex-1 btn btn-secondary text-xs"
              >
                Later
              </button>
              <button onClick={handleInstall} className="flex-1 btn btn-primary text-xs">
                Install &amp; restart
              </button>
            </div>
          </>
        )}

        {phase === 'downloading' && (
          <div>
            <div className="flex items-center gap-2 text-xs text-slate-300 mb-2">
              <Loader2 size={14} className="animate-spin text-blue-400" />
              Downloading…
            </div>
            <div className="h-1.5 bg-slate-800 rounded overflow-hidden">
              <div
                className={`h-full bg-blue-500 transition-all ${
                  pct === null ? 'animate-pulse' : ''
                }`}
                style={{ width: pct !== null ? `${pct}%` : '100%' }}
              />
            </div>
            <div className="text-[10px] text-slate-500 mt-1 font-mono">
              {humanBytes(progress.done)}
              {progress.total ? ` / ${humanBytes(progress.total)}` : ''}
            </div>
          </div>
        )}

        {phase === 'installing' && (
          <div className="flex items-center gap-2 text-xs text-slate-300">
            <Loader2 size={14} className="animate-spin text-blue-400" />
            Installing the new version…
          </div>
        )}

        {phase === 'done' && (
          <div className="flex items-center gap-2 text-xs text-emerald-400">
            <CheckCircle2 size={14} />
            Update applied — restarting…
          </div>
        )}

        {phase === 'error' && (
          <div className="flex items-start gap-2 text-xs text-red-400">
            <AlertCircle size={14} className="shrink-0 mt-0.5" />
            <div>
              <div className="font-medium mb-0.5">Update failed</div>
              <div className="text-slate-500 break-all">{errorMsg}</div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

function humanBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  return `${(n / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}
