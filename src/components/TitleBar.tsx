import { useState, useEffect } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Minus, Square, X, Copy } from 'lucide-react';

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

function BrandGlyph() {
  return (
    <div
      className="relative w-5 h-5 rounded-md flex items-center justify-center pointer-events-none"
      style={{
        background:
          'linear-gradient(135deg, #3b82f6 0%, #6366f1 50%, #8b5cf6 100%)',
        boxShadow:
          '0 0 0 1px rgba(99, 102, 241, 0.35), 0 2px 6px -1px rgba(99, 102, 241, 0.5)',
      }}
    >
      {/* LocalForge mark — L-bracket + forge spark.
       * Matches localforge-cloud/brand/logo-mark.svg. */}
      <svg width="20" height="20" viewBox="0 0 32 32" fill="none">
        <path
          d="M 10 7 L 10 21 Q 10 25 14 25 L 25 25"
          stroke="white"
          strokeWidth="3.5"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
        <circle cx="26" cy="6" r="2.2" fill="#fde68a" />
      </svg>
    </div>
  );
}
