import { useState, useEffect, useRef } from 'react';
import { useNavigate } from 'react-router-dom';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Minus, Square, X, Copy, LogIn, ChevronDown, Check } from 'lucide-react';
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

      {/* Org switcher — only rendered when the user belongs to >1 org
          (most users have exactly one personal workspace). */}
      <OrgSwitcher />

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

function OrgSwitcher() {
  const orgs = useAuthStore((s) => s.orgs);
  const currentOrgId = useAuthStore((s) => s.currentOrgId);
  const setCurrentOrg = useAuthStore((s) => s.setCurrentOrg);
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onClick = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', onClick);
    return () => document.removeEventListener('mousedown', onClick);
  }, [open]);

  // Hide entirely when only one (or none) — the chip is noise then.
  if (orgs.length <= 1) return null;
  const current = orgs.find((o) => o.id === currentOrgId);
  if (!current) return null;

  return (
    <div className="title-bar-org" ref={ref}>
      <button
        className="title-bar-org-btn"
        onClick={() => setOpen((v) => !v)}
        title={`Active workspace: ${current.name} (${current.role})`}
      >
        <span className="title-bar-org-name">{current.name}</span>
        {!current.isOwner && (
          <span className={`title-bar-plan role-${current.role}`}>{current.role}</span>
        )}
        <ChevronDown size={11} strokeWidth={2.2} />
      </button>
      {open && (
        <div className="title-bar-org-menu">
          <div className="title-bar-org-menu-label">Workspaces</div>
          {orgs.map((o) => (
            <button
              key={o.id}
              className={`title-bar-org-menu-item${o.id === currentOrgId ? ' active' : ''}`}
              onClick={() => { setCurrentOrg(o.id); setOpen(false); }}
            >
              <div className="title-bar-org-menu-info">
                <span className="title-bar-org-menu-name">{o.name}</span>
                <span className="title-bar-org-menu-meta">
                  {o.isOwner ? 'Owner' : o.role}
                </span>
              </div>
              {o.id === currentOrgId && <Check size={12} strokeWidth={2.4} />}
            </button>
          ))}
        </div>
      )}
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
  // LocalForge "Crucible" mark — a hexagonal crucible with a molten ember
  // ingot + steel-forged edge, and two nodes: a filled ember stud (local) +
  // a hollow steel ring (remote). Local vs remote reads by SHAPE as well as
  // colour. Matches favicon.svg + the mobile/landing brand.
  return (
    <svg
      width="20"
      height="20"
      viewBox="0 0 120 120"
      fill="none"
      className="pointer-events-none"
      aria-label="LocalForge"
    >
      <defs>
        <linearGradient id="tb-steel" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#7dd3fc" />
          <stop offset="1" stopColor="#38bdf8" />
        </linearGradient>
        <linearGradient id="tb-ingot" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#fdba74" />
          <stop offset="0.45" stopColor="#f97316" />
          <stop offset="1" stopColor="#c2410c" />
        </linearGradient>
        <radialGradient id="tb-stud" cx="0.36" cy="0.32" r="0.8">
          <stop offset="0" stopColor="#fdba74" />
          <stop offset="0.55" stopColor="#f97316" />
          <stop offset="1" stopColor="#c2410c" />
        </radialGradient>
        <radialGradient id="tb-glow" cx="0.5" cy="0.5" r="0.5">
          <stop offset="0" stopColor="#f97316" stopOpacity="0.6" />
          <stop offset="0.6" stopColor="#f97316" stopOpacity="0.12" />
          <stop offset="1" stopColor="#f97316" stopOpacity="0" />
        </radialGradient>
      </defs>
      <rect x="2" y="2" width="116" height="116" rx="30" fill="#0d1422" stroke="rgba(148,163,184,0.16)" strokeWidth="1.5" />
      <g transform="translate(28 28) scale(0.64)">
        <polygon points="50,8 86.4,29 86.4,71 50,92 13.6,71 13.6,29" fill="#0d1422" stroke="url(#tb-steel)" strokeWidth="6" strokeLinejoin="round" />
        <ellipse cx="50" cy="58" rx="30" ry="24" fill="url(#tb-glow)" />
        <polygon points="13.6,55 86.4,55 86.4,71 50,92 13.6,71" fill="url(#tb-ingot)" />
        <rect x="15" y="53.4" width="70" height="3.4" rx="1.7" fill="#fdba74" opacity="0.8" />
        <circle cx="13.6" cy="50" r="9.5" fill="url(#tb-stud)" stroke="#0a0d14" strokeWidth="3" />
        <circle cx="86.4" cy="50" r="9.5" fill="#0a0d14" stroke="url(#tb-steel)" strokeWidth="5.5" />
      </g>
    </svg>
  );
}
