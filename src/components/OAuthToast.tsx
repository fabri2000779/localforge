import { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { ExternalLink, X } from 'lucide-react';

/**
 * Transient banner shown when the install pipeline opened an OAuth URL
 * in the user's local browser. The desktop already spawned `xdg-open` /
 * `open` / `start`, so this is purely informational — it tells the user
 * "go check your browser, complete the login, come back".
 *
 * We listen for `install-oauth-opened` Tauri events emitted from
 * commands::server::run_install_pipeline.
 */
export function OAuthToast() {
  const [items, setItems] = useState<{ id: number; url: string }[]>([]);

  useEffect(() => {
    let id = 0;
    const unlisten = listen<{ url: string }>(
      'install-oauth-opened',
      (event) => {
        const myId = ++id;
        const url = event.payload.url;
        setItems((prev) => {
          // Dedupe — don't stack multiple toasts for the same URL.
          if (prev.some((t) => t.url === url)) return prev;
          return [...prev, { id: myId, url }];
        });
        // Auto-dismiss after 30s.
        setTimeout(() => {
          setItems((prev) => prev.filter((t) => t.id !== myId));
        }, 30_000);
      },
    );

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  if (items.length === 0) return null;

  return (
    <div className="fixed top-12 right-4 z-50 space-y-2 max-w-sm">
      {items.map((t) => (
        <div
          key={t.id}
          className="bg-slate-800 border border-indigo-500/30 rounded-lg shadow-xl p-4 animate-fade-in"
        >
          <div className="flex items-start gap-3">
            <ExternalLink size={18} className="text-indigo-400 shrink-0 mt-0.5" />
            <div className="flex-1 min-w-0">
              <div className="font-semibold text-sm mb-1">
                Auth URL opened in your browser
              </div>
              <p className="text-xs text-slate-400 mb-2">
                Complete the login there and the install will continue.
              </p>
              <div className="text-xs font-mono text-slate-500 truncate">
                {t.url}
              </div>
            </div>
            <button
              onClick={() =>
                setItems((prev) => prev.filter((x) => x.id !== t.id))
              }
              className="text-slate-500 hover:text-white shrink-0"
              aria-label="Dismiss"
            >
              <X size={16} />
            </button>
          </div>
        </div>
      ))}
    </div>
  );
}
