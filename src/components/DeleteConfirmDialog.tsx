import { useState } from 'react';
import { Trash2, X, AlertTriangle, Folder } from 'lucide-react';

interface DeleteConfirmDialogProps {
  isOpen: boolean;
  serverName: string;
  dataPath: string;
  onConfirm: (deleteData: boolean) => void;
  onCancel: () => void;
}

export function DeleteConfirmDialog({ 
  isOpen, 
  serverName, 
  dataPath, 
  onConfirm, 
  onCancel 
}: DeleteConfirmDialogProps) {
  const [deleteData, setDeleteData] = useState(true);

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      {/* Backdrop */}
      <div 
        className="absolute inset-0 bg-black/70 backdrop-blur-sm"
        onClick={onCancel}
      />
      
      {/* Dialog */}
      <div className="relative bg-zinc-900 border border-zinc-800 rounded-xl shadow-2xl max-w-md w-full mx-4 animate-fade-in">
        {/* Header */}
        <div className="flex items-center gap-3 p-4 border-b border-zinc-800">
          <div className="w-10 h-10 rounded-full bg-red-500/20 flex items-center justify-center">
            <AlertTriangle size={20} className="text-red-500" />
          </div>
          <div className="flex-1">
            <h3 className="font-semibold text-lg">Delete Server</h3>
            <p className="text-sm text-zinc-400">This action cannot be undone</p>
          </div>
          <button 
            onClick={onCancel}
            className="p-2 hover:bg-zinc-800 rounded-lg transition-colors"
          >
            <X size={20} className="text-zinc-400" />
          </button>
        </div>
        
        {/* Content */}
        <div className="p-4 space-y-4">
          <p className="text-zinc-300">
            Are you sure you want to delete <span className="font-semibold text-white">"{serverName}"</span>?
          </p>
          
          {/* Delete data option */}
          <div 
            onClick={() => setDeleteData(!deleteData)}
            className={`p-4 rounded-lg border cursor-pointer transition-colors ${
              deleteData 
                ? 'bg-red-950/30 border-red-900/50' 
                : 'bg-zinc-800/50 border-zinc-700 hover:border-zinc-600'
            }`}
          >
            <div className="flex items-start gap-3">
              <div className={`w-5 h-5 rounded border-2 flex items-center justify-center mt-0.5 transition-colors ${
                deleteData 
                  ? 'bg-red-500 border-red-500' 
                  : 'border-zinc-500'
              }`}>
                {deleteData && (
                  <svg className="w-3 h-3 text-white" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={3} d="M5 13l4 4L19 7" />
                  </svg>
                )}
              </div>
              <div className="flex-1">
                <div className="font-medium flex items-center gap-2">
                  <Trash2 size={16} className={deleteData ? 'text-red-400' : 'text-zinc-400'} />
                  Delete world data
                </div>
                <p className="text-sm text-zinc-400 mt-1">
                  {deleteData 
                    ? 'All server files will be permanently deleted' 
                    : 'Server files will be preserved for later use'
                  }
                </p>
              </div>
            </div>
          </div>
          
          {/* Data path info */}
          <div className="flex items-start gap-2 p-3 bg-zinc-800/50 rounded-lg">
            <Folder size={16} className="text-zinc-500 mt-0.5 flex-shrink-0" />
            <div className="text-xs">
              <span className="text-zinc-500">Data location: </span>
              <code className="text-zinc-400 break-all">{dataPath}</code>
            </div>
          </div>
        </div>
        
        {/* Actions */}
        <div className="flex items-center gap-3 p-4 border-t border-zinc-800 bg-zinc-900/50 rounded-b-xl">
          <button 
            onClick={onCancel}
            className="btn btn-secondary flex-1"
          >
            Cancel
          </button>
          <button 
            onClick={() => onConfirm(deleteData)}
            className="btn btn-danger flex-1"
          >
            <Trash2 size={18} />
            {deleteData ? 'Delete Everything' : 'Delete Server'}
          </button>
        </div>
      </div>
    </div>
  );
}
