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
          className="rounded-xl shadow-2xl p-4 backdrop-blur-md animate-fade-in"
          style={{
            background:
              'linear-gradient(180deg, rgba(20, 26, 38, 0.95) 0%, rgba(14, 19, 28, 0.95) 100%)',
            border: '1px solid rgba(139, 92, 246, 0.32)',
            boxShadow:
              '0 0 0 1px rgba(139, 92, 246, 0.05), 0 24px 50px -20px rgba(0, 0, 0, 0.7)',
          }}
        >
          <div className="flex items-start gap-3">
            <div
              className="w-9 h-9 rounded-lg shrink-0 flex items-center justify-center"
              style={{
                background:
                  'linear-gradient(135deg, rgba(99, 102, 241, 0.2), rgba(139, 92, 246, 0.2))',
                boxShadow: '0 0 0 1px rgba(139, 92, 246, 0.18)',
              }}
            >
              <ExternalLink size={15} className="text-indigo-300" />
            </div>
            <div className="flex-1 min-w-0">
              <div className="font-semibold text-[13px] text-slate-100 tracking-tight mb-0.5">
                Auth URL opened in your browser
              </div>
              <p className="text-[11.5px] text-slate-400 mb-2 leading-relaxed">
                Complete the login there and the install will continue.
              </p>
              <div className="text-[10.5px] font-mono text-slate-500 truncate">
                {t.url}
              </div>
            </div>
            <button
              onClick={() =>
                setItems((prev) => prev.filter((x) => x.id !== t.id))
              }
              className="text-slate-500 hover:text-white shrink-0 -m-1 p-1 rounded transition-colors hover:bg-slate-800"
              aria-label="Dismiss"
            >
              <X size={14} />
            </button>
          </div>
        </div>
      ))}
    </div>
  );
}
