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
      unlisten.then(fn => fn());
    };
  }, []);

  const handleMinimize = () => getCurrentWindow().minimize();
  const handleMaximize = () => getCurrentWindow().toggleMaximize();
  const handleClose = () => getCurrentWindow().close();

  return (
    <div 
      className="h-9 bg-slate-900 flex items-center select-none border-b border-slate-800/50"
      data-tauri-drag-region
    >
      {/* Left side - App branding (acts as drag region) */}
      <div className="flex items-center gap-2 px-4 h-full" data-tauri-drag-region>
        <span className="text-sm text-slate-200 font-semibold pointer-events-none">LocalForge</span>
      </div>

      {/* Spacer - drag region */}
      <div className="flex-1 h-full" data-tauri-drag-region />

      {/* Window Controls */}
      <div className="flex h-full">
        <button
          onClick={handleMinimize}
          className="w-12 h-full flex items-center justify-center text-slate-400 hover:bg-slate-700 hover:text-white transition-colors"
        >
          <Minus size={16} />
        </button>
        <button
          onClick={handleMaximize}
          className="w-12 h-full flex items-center justify-center text-slate-400 hover:bg-slate-700 hover:text-white transition-colors"
        >
          {isMaximized ? <Copy size={12} className="scale-x-[-1]" /> : <Square size={12} />}
        </button>
        <button
          onClick={handleClose}
          className="w-12 h-full flex items-center justify-center text-slate-400 hover:bg-red-600 hover:text-white transition-colors"
        >
          <X size={16} />
        </button>
      </div>
    </div>
  );
}
