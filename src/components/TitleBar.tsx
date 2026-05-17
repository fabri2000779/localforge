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
      {/* Stylised "L" anvil — sharp + minimal */}
      <svg width="11" height="11" viewBox="0 0 11 11" fill="none">
        <path
          d="M3 1.5v6.2c0 .4.3.8.8.8H9"
          stroke="white"
          strokeWidth="1.5"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      </svg>
    </div>
  );
}
