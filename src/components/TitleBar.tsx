import { useState, useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Minus, Square, X, Copy, LogIn } from 'lucide-react';
import { useAuthStore } from '../stores/authStore';
import { LoginDialog } from './LoginDialog';

export function TitleBar() {
  const [isMaximized, setIsMaximized] = useState(false);

  useEffect(() => {
    const appWindow = getCurrentWindow();
    appWindow.isMaximized().then(setIsMaximized);

    const unlisten = appWindow.onResized(() => {
      appWindow.isMaximized().then(setIsMaximized);
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const handleMinimize = () => getCurrentWindow().minimize();
  const handleMaximize = () => getCurrentWindow().toggleMaximize();
  const handleClose = () => getCurrentWindow().close();

  return (
    <div className="title-bar" data-tauri-drag-region>
      {/* Left — Brand */}
      <div
        className="flex items-center gap-2.5 px-3.5 h-full"
        data-tauri-drag-region
      >
        <BrandGlyph />
        <span className="text-[13px] font-semibold tracking-tight pointer-events-none">
          LocalForge
        </span>
      </div>

      {/* Spacer */}
      <div className="flex-1 h-full" data-tauri-drag-region />

      {/* Cloud account chip — "Sign in" when signed-out, name+plan when signed-in.
          Both routes the user toward the right surface; the in-depth UI lives
          on the Settings page. */}
      <AccountChip />

      {/* Window Controls */}
      <div className="flex h-full">
        <button
          onClick={handleMinimize}
          className="title-bar-btn"
          aria-label="Minimize"
        >
          <Minus size={14} strokeWidth={2.2} />
        </button>
        <button
          onClick={handleMaximize}
          className="title-bar-btn"
          aria-label={isMaximized ? 'Restore' : 'Maximize'}
        >
          {isMaximized ? (
            <Copy size={11} strokeWidth={2.2} className="scale-x-[-1]" />
          ) : (
            <Square size={11} strokeWidth={2.2} />
          )}
        </button>
        <button
          onClick={handleClose}
          className="title-bar-btn title-bar-btn-close"
          aria-label="Close"
        >
          <X size={14} strokeWidth={2.2} />
        </button>
      </div>
    </div>
  );
}

function AccountChip() {
  const me = useAuthStore((s) => s.me);
  const navigate = useNavigate();
  const [open, setOpen] = useState(false);

  // Signed-out (or not-yet-checked) chip → opens the modal directly.
  if (me === null || me === undefined) {
    return (
      <>
        <button
          className="title-bar-account"
          onClick={() => setOpen(true)}
          title="Sign in to your LocalForge cloud account"
        >
          <LogIn size={12} strokeWidth={2.2} />
          <span>Sign in</span>
        </button>
        <LoginDialog open={open} onClose={() => setOpen(false)} />
      </>
    );
  }

  // Signed-in chip — shows initials + plan badge, click navigates to
  // the Settings page where the full Cloud Account panel lives.
  const initials = (me.displayName ?? me.email).slice(0, 2).toUpperCase();
  const plan = me.subscription.plan;
  return (
    <button
      onClick={() => navigate('/settings')}
      className="title-bar-account"
      title={`${me.email} · ${plan}`}
    >
      <span className="title-bar-account-avatar">{initials}</span>
      <span className="hidden sm:inline truncate max-w-[120px]">{me.displayName ?? me.email}</span>
      {plan !== 'free' && <span className={`title-bar-plan plan-${plan}`}>{plan === 'team' ? 'Team' : 'Hobby'}</span>}
    </button>
  );
}

function BrandGlyph() {
  return (
    <div
      className="relative w-5 h-5 rounded-md flex items-center justify-center pointer-events-none overflow-hidden"
      style={{
        background:
          'linear-gradient(135deg, #101827 0%, #07090f 55%, #0b1020 100%)',
        boxShadow:
          '0 0 0 1px rgba(99, 102, 241, 0.30), 0 2px 6px -1px rgba(99, 102, 241, 0.40)',
      }}
    >
      {/* LocalForge mark — hex container + LF monogram + 2 endpoint dots.
       * Simplified variant of localforge-cloud/brand/logo-mark.svg
       * tuned for ~20px chrome. Matches favicon.svg. */}
      <svg width="20" height="20" viewBox="0 0 32 32" fill="none">
        {/* Hex container */}
        <path
          d="M16 4 L26 9.5 V22.5 L16 28 L6 22.5 V9.5 Z"
          stroke="#60a5fa"
          strokeWidth="1.6"
          strokeLinejoin="round"
        />
        {/* L stroke */}
        <path
          d="M11.5 10 v8 a2 2 0 0 0 2 2 h3.5"
          stroke="#60a5fa"
          strokeWidth="2.2"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
        {/* F stroke */}
        <path
          d="M17.5 21 V11.5 h5 M17.5 16 h3.8"
          stroke="#a78bfa"
          strokeWidth="2.2"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
        {/* Endpoint dots: local (cyan) + remote (violet) */}
        <circle cx="9" cy="22" r="1.4" fill="#38bdf8" />
        <circle cx="23" cy="11" r="1.4" fill="#a78bfa" />
      </svg>
    </div>
  );
}
