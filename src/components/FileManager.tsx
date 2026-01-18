import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-shell';
import {
  Folder,
  File,
  ChevronRight,
  ChevronLeft,
  RefreshCw,
  Plus,
  Trash2,
  Edit3,
  Copy,
  Scissors,
  FolderPlus,
  FilePlus,
  MoreVertical,
  X,
  Save,
  Home,
  ArrowUp,
  FolderOpen,
  FileText,
  FileCode,
  FileJson,
  Image,
  Archive,
  Database,
  Settings,
  Check,
} from 'lucide-react';

interface FileEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
  modified: number | null;
  extension: string | null;
}

interface DirectoryContents {
  path: string;
  parent: string | null;
  entries: FileEntry[];
}

interface FileManagerProps {
  rootPath: string;
  serverName?: string;
}

// File icons based on extension
const getFileIcon = (entry: FileEntry) => {
  if (entry.is_dir) {
    return <Folder size={18} className="text-yellow-400" />;
  }
  
  const ext = entry.extension?.toLowerCase();
  switch (ext) {
    case 'json':
      return <FileJson size={18} className="text-yellow-500" />;
    case 'yml':
    case 'yaml':
    case 'toml':
      return <Settings size={18} className="text-purple-400" />;
    case 'properties':
    case 'cfg':
    case 'conf':
    case 'ini':
      return <FileCode size={18} className="text-blue-400" />;
    case 'txt':
    case 'log':
    case 'md':
      return <FileText size={18} className="text-zinc-400" />;
    case 'js':
    case 'ts':
    case 'java':
    case 'py':
    case 'sh':
    case 'bat':
      return <FileCode size={18} className="text-green-400" />;
    case 'png':
    case 'jpg':
    case 'jpeg':
    case 'gif':
    case 'webp':
    case 'svg':
    case 'ico':
      return <Image size={18} className="text-pink-400" />;
    case 'zip':
    case 'tar':
    case 'gz':
    case 'rar':
    case '7z':
      return <Archive size={18} className="text-orange-400" />;
    case 'jar':
      return <Database size={18} className="text-red-400" />;
    case 'dat':
    case 'db':
    case 'sqlite':
      return <Database size={18} className="text-cyan-400" />;
    default:
      return <File size={18} className="text-zinc-500" />;
  }
};

// Check if file is editable (text-based)
const isEditable = (entry: FileEntry): boolean => {
  if (entry.is_dir) return false;
  
  const editableExtensions = [
    'txt', 'log', 'md', 'json', 'yml', 'yaml', 'toml', 'xml',
    'properties', 'cfg', 'conf', 'ini', 'env',
    'js', 'ts', 'jsx', 'tsx', 'css', 'scss', 'html',
    'java', 'py', 'sh', 'bat', 'ps1', 'cmd',
    'sql', 'csv', 'htaccess',
  ];
  
  const ext = entry.extension?.toLowerCase();
  return ext ? editableExtensions.includes(ext) : false;
};

// Format file size
const formatSize = (bytes: number): string => {
  if (bytes === 0) return '—';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
};

// Format date
const formatDate = (timestamp: number | null): string => {
  if (!timestamp) return '—';
  const date = new Date(timestamp * 1000);
  return date.toLocaleDateString() + ' ' + date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
};

export function FileManager({ rootPath, serverName }: FileManagerProps) {
  const [currentPath, setCurrentPath] = useState(rootPath);
  const [contents, setContents] = useState<DirectoryContents | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  
  // Selection state
  const [selectedItems, setSelectedItems] = useState<Set<string>>(new Set());
  const [clipboard, setClipboard] = useState<{ paths: string[]; operation: 'copy' | 'cut' } | null>(null);
  
  // Context menu
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; entry: FileEntry | null } | null>(null);
  
  // Dialogs
  const [renameDialog, setRenameDialog] = useState<{ entry: FileEntry; newName: string } | null>(null);
  const [newItemDialog, setNewItemDialog] = useState<{ type: 'file' | 'folder'; name: string } | null>(null);
  const [deleteConfirm, setDeleteConfirm] = useState<FileEntry[] | null>(null);
  
  // Editor
  const [editingFile, setEditingFile] = useState<{ path: string; name: string; content: string; original: string } | null>(null);
  const [editorSaving, setEditorSaving] = useState(false);

  // Load directory contents
  const loadDirectory = useCallback(async (path: string) => {
    setLoading(true);
    setError(null);
    setSelectedItems(new Set());
    
    try {
      const result = await invoke<DirectoryContents>('list_directory', { path });
      setContents(result);
      setCurrentPath(path);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  // Initial load
  useEffect(() => {
    loadDirectory(rootPath);
  }, [rootPath, loadDirectory]);

  // Auto-refresh every 3 seconds for live updates
  useEffect(() => {
    const interval = setInterval(() => {
      if (!editingFile && !renameDialog && !newItemDialog && !deleteConfirm) {
        // Silent refresh - don't show loading state
        invoke<DirectoryContents>('list_directory', { path: currentPath })
          .then(result => {
            // Only update if entries changed (compare by JSON)
            const currentEntries = JSON.stringify(contents?.entries.map(e => ({ name: e.name, size: e.size, modified: e.modified })));
            const newEntries = JSON.stringify(result.entries.map(e => ({ name: e.name, size: e.size, modified: e.modified })));
            if (currentEntries !== newEntries) {
              setContents(result);
            }
          })
          .catch(() => {}); // Ignore errors on auto-refresh
      }
    }, 3000);

    return () => clearInterval(interval);
  }, [currentPath, contents, editingFile, renameDialog, newItemDialog, deleteConfirm]);

  // Close context menu on click outside
  useEffect(() => {
    const handleClick = () => setContextMenu(null);
    document.addEventListener('click', handleClick);
    return () => document.removeEventListener('click', handleClick);
  }, []);

  // Navigate to directory
  const navigateTo = (path: string) => {
    loadDirectory(path);
  };

  // Go up one level
  const goUp = () => {
    if (contents?.parent && currentPath !== rootPath) {
      // Don't go above root
      if (contents.parent.startsWith(rootPath) || contents.parent === rootPath.replace(/[\\/]$/, '')) {
        navigateTo(contents.parent);
      }
    }
  };

  // Go to root
  const goHome = () => {
    navigateTo(rootPath);
  };

  // Open file/folder
  const handleOpen = async (entry: FileEntry) => {
    if (entry.is_dir) {
      navigateTo(entry.path);
    } else if (isEditable(entry)) {
      // Open in editor
      try {
        const content = await invoke<string>('read_file_text', { path: entry.path });
        setEditingFile({ path: entry.path, name: entry.name, content, original: content });
      } catch (e) {
        setError(String(e));
      }
    }
  };

  // Handle selection
  const handleSelect = (entry: FileEntry, e: React.MouseEvent) => {
    if (e.ctrlKey || e.metaKey) {
      // Toggle selection
      const newSelection = new Set(selectedItems);
      if (newSelection.has(entry.path)) {
        newSelection.delete(entry.path);
      } else {
        newSelection.add(entry.path);
      }
      setSelectedItems(newSelection);
    } else if (e.shiftKey && selectedItems.size > 0) {
      // Range selection
      const entries = contents?.entries || [];
      const lastSelected = Array.from(selectedItems).pop();
      const lastIndex = entries.findIndex(e => e.path === lastSelected);
      const currentIndex = entries.findIndex(e => e.path === entry.path);
      
      if (lastIndex !== -1 && currentIndex !== -1) {
        const start = Math.min(lastIndex, currentIndex);
        const end = Math.max(lastIndex, currentIndex);
        const newSelection = new Set(selectedItems);
        for (let i = start; i <= end; i++) {
          newSelection.add(entries[i].path);
        }
        setSelectedItems(newSelection);
      }
    } else {
      setSelectedItems(new Set([entry.path]));
    }
  };

  // Context menu handler
  const handleContextMenu = (e: React.MouseEvent, entry: FileEntry | null) => {
    e.preventDefault();
    e.stopPropagation();
    setContextMenu({ x: e.clientX, y: e.clientY, entry });
  };

  // Delete items
  const handleDelete = async () => {
    if (!deleteConfirm) return;
    
    try {
      for (const entry of deleteConfirm) {
        await invoke('delete_path', { path: entry.path });
      }
      setDeleteConfirm(null);
      loadDirectory(currentPath);
    } catch (e) {
      setError(String(e));
    }
  };

  // Rename item
  const handleRename = async () => {
    if (!renameDialog) return;
    
    try {
      await invoke('rename_path', { oldPath: renameDialog.entry.path, newName: renameDialog.newName });
      setRenameDialog(null);
      loadDirectory(currentPath);
    } catch (e) {
      setError(String(e));
    }
  };

  // Create new item
  const handleCreateNew = async () => {
    if (!newItemDialog) return;
    
    const newPath = `${currentPath}/${newItemDialog.name}`;
    
    try {
      if (newItemDialog.type === 'folder') {
        await invoke('create_directory', { path: newPath });
      } else {
        await invoke('create_file', { path: newPath, content: '' });
      }
      setNewItemDialog(null);
      loadDirectory(currentPath);
    } catch (e) {
      setError(String(e));
    }
  };

  // Copy/Cut to clipboard
  const handleCopy = (cut: boolean = false) => {
    const paths = Array.from(selectedItems);
    if (paths.length > 0) {
      setClipboard({ paths, operation: cut ? 'cut' : 'copy' });
    }
    setContextMenu(null);
  };

  // Paste from clipboard
  const handlePaste = async () => {
    if (!clipboard) return;
    
    try {
      for (const path of clipboard.paths) {
        if (clipboard.operation === 'copy') {
          await invoke('copy_path', { source: path, destinationDir: currentPath });
        } else {
          await invoke('move_path', { source: path, destinationDir: currentPath });
        }
      }
      
      if (clipboard.operation === 'cut') {
        setClipboard(null);
      }
      
      loadDirectory(currentPath);
    } catch (e) {
      setError(String(e));
    }
    setContextMenu(null);
  };

  // Save edited file
  const handleSaveFile = async () => {
    if (!editingFile) return;
    
    setEditorSaving(true);
    try {
      await invoke('write_file_text', { path: editingFile.path, content: editingFile.content });
      setEditingFile({ ...editingFile, original: editingFile.content });
    } catch (e) {
      setError(String(e));
    } finally {
      setEditorSaving(false);
    }
  };

  // Close editor
  const handleCloseEditor = () => {
    if (editingFile && editingFile.content !== editingFile.original) {
      if (!confirm('You have unsaved changes. Close anyway?')) {
        return;
      }
    }
    setEditingFile(null);
  };

  // Open in system file explorer
  const openInExplorer = async () => {
    try {
      await open(currentPath);
    } catch (e) {
      console.error('Failed to open folder:', e);
    }
  };

  // Breadcrumb path
  const getBreadcrumbs = () => {
    const relativePath = currentPath.replace(rootPath, '').replace(/^[\\/]/, '');
    const parts = relativePath ? relativePath.split(/[\\/]/) : [];
    
    const crumbs = [{ name: serverName || 'Server', path: rootPath }];
    let accPath = rootPath;
    
    for (const part of parts) {
      accPath = `${accPath}/${part}`;
      crumbs.push({ name: part, path: accPath });
    }
    
    return crumbs;
  };

  // Get selected entries
  const getSelectedEntries = (): FileEntry[] => {
    return contents?.entries.filter(e => selectedItems.has(e.path)) || [];
  };

  // File editor modal
  if (editingFile) {
    const hasChanges = editingFile.content !== editingFile.original;
    
    return (
      <div className="h-full flex flex-col bg-zinc-900 rounded-lg overflow-hidden">
        {/* Editor Header */}
        <div className="flex items-center justify-between px-4 py-3 bg-zinc-800 border-b border-zinc-700">
          <div className="flex items-center gap-3">
            <FileCode size={18} className="text-blue-400" />
            <span className="font-medium">{editingFile.name}</span>
            {hasChanges && <span className="text-xs text-yellow-400">• Modified</span>}
          </div>
          <div className="flex items-center gap-2">
            <button
              onClick={handleSaveFile}
              disabled={editorSaving || !hasChanges}
              className="btn btn-sm btn-success"
            >
              {editorSaving ? <RefreshCw size={14} className="animate-spin" /> : <Save size={14} />}
              Save
            </button>
            <button onClick={handleCloseEditor} className="btn btn-sm btn-secondary">
              <X size={14} /> Close
            </button>
          </div>
        </div>
        
        {/* Editor Content */}
        <textarea
          value={editingFile.content}
          onChange={(e) => setEditingFile({ ...editingFile, content: e.target.value })}
          className="flex-1 w-full bg-zinc-950 text-zinc-200 font-mono text-sm p-4 resize-none focus:outline-none"
          spellCheck={false}
        />
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col bg-zinc-900 rounded-lg overflow-hidden">
      {/* Toolbar */}
      <div className="flex items-center gap-2 px-3 py-2 bg-zinc-800 border-b border-zinc-700">
        <button
          onClick={goUp}
          disabled={currentPath === rootPath}
          className="p-1.5 rounded hover:bg-zinc-700 disabled:opacity-30 disabled:cursor-not-allowed"
          title="Go up"
        >
          <ArrowUp size={18} />
        </button>
        <button
          onClick={goHome}
          disabled={currentPath === rootPath}
          className="p-1.5 rounded hover:bg-zinc-700 disabled:opacity-30 disabled:cursor-not-allowed"
          title="Go to root"
        >
          <Home size={18} />
        </button>
        <button
          onClick={() => loadDirectory(currentPath)}
          disabled={loading}
          className="p-1.5 rounded hover:bg-zinc-700"
          title="Refresh"
        >
          <RefreshCw size={18} className={loading ? 'animate-spin' : ''} />
        </button>
        
        <div className="w-px h-5 bg-zinc-700 mx-1" />
        
        <button
          onClick={() => setNewItemDialog({ type: 'file', name: '' })}
          className="p-1.5 rounded hover:bg-zinc-700"
          title="New file"
        >
          <FilePlus size={18} />
        </button>
        <button
          onClick={() => setNewItemDialog({ type: 'folder', name: '' })}
          className="p-1.5 rounded hover:bg-zinc-700"
          title="New folder"
        >
          <FolderPlus size={18} />
        </button>
        
        {selectedItems.size > 0 && (
          <>
            <div className="w-px h-5 bg-zinc-700 mx-1" />
            <button
              onClick={() => handleCopy(false)}
              className="p-1.5 rounded hover:bg-zinc-700"
              title="Copy"
            >
              <Copy size={18} />
            </button>
            <button
              onClick={() => handleCopy(true)}
              className="p-1.5 rounded hover:bg-zinc-700"
              title="Cut"
            >
              <Scissors size={18} />
            </button>
            <button
              onClick={() => setDeleteConfirm(getSelectedEntries())}
              className="p-1.5 rounded hover:bg-zinc-700 text-red-400"
              title="Delete"
            >
              <Trash2 size={18} />
            </button>
          </>
        )}
        
        {clipboard && (
          <>
            <div className="w-px h-5 bg-zinc-700 mx-1" />
            <button
              onClick={handlePaste}
              className="p-1.5 rounded hover:bg-zinc-700 text-green-400"
              title={`Paste (${clipboard.paths.length} items)`}
            >
              <Check size={18} />
            </button>
            <span className="text-xs text-zinc-500">
              {clipboard.paths.length} {clipboard.operation === 'cut' ? 'to move' : 'to paste'}
            </span>
          </>
        )}
        
        <div className="flex-1" />
        
        <button onClick={openInExplorer} className="btn btn-sm btn-secondary">
          <FolderOpen size={14} /> Open in Explorer
        </button>
      </div>
      
      {/* Breadcrumbs */}
      <div className="flex items-center gap-1 px-3 py-2 bg-zinc-800/80 border-b border-zinc-700 text-sm overflow-x-auto">
        {getBreadcrumbs().map((crumb, i, arr) => (
          <span key={crumb.path} className="flex items-center gap-1 whitespace-nowrap">
            {i > 0 && <ChevronRight size={14} className="text-zinc-600" />}
            <button
              onClick={() => navigateTo(crumb.path)}
              className={`px-1.5 py-0.5 rounded hover:bg-zinc-700 ${
                i === arr.length - 1 ? 'text-white font-medium' : 'text-zinc-400'
              }`}
            >
              {crumb.name}
            </button>
          </span>
        ))}
      </div>
      
      {/* Error */}
      {error && (
        <div className="px-3 py-2 bg-red-500/10 border-b border-red-500/30 text-sm text-red-400 flex items-center justify-between">
          <span>{error}</span>
          <button onClick={() => setError(null)} className="p-1 hover:bg-red-500/20 rounded">
            <X size={14} />
          </button>
        </div>
      )}
      
      {/* File List */}
      <div 
        className="flex-1 overflow-auto"
        onContextMenu={(e) => handleContextMenu(e, null)}
        onClick={() => setSelectedItems(new Set())}
      >
        {loading ? (
          <div className="flex items-center justify-center h-32 text-zinc-500">
            <RefreshCw size={20} className="animate-spin mr-2" /> Loading...
          </div>
        ) : contents?.entries.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-32 text-zinc-500">
            <Folder size={32} className="mb-2 opacity-50" />
            <span>This folder is empty</span>
          </div>
        ) : (
          <table className="w-full text-sm">
            <thead className="bg-zinc-800/50 sticky top-0">
              <tr className="text-left text-zinc-400">
                <th className="px-3 py-2 font-medium">Name</th>
                <th className="px-3 py-2 font-medium w-24">Size</th>
                <th className="px-3 py-2 font-medium w-40">Modified</th>
              </tr>
            </thead>
            <tbody>
              {contents?.entries.map((entry) => (
                <tr
                  key={entry.path}
                  className={`hover:bg-zinc-800/50 cursor-pointer border-b border-zinc-800/50 ${
                    selectedItems.has(entry.path) ? 'bg-indigo-500/20' : ''
                  }`}
                  onClick={(e) => {
                    e.stopPropagation();
                    handleSelect(entry, e);
                  }}
                  onDoubleClick={() => handleOpen(entry)}
                  onContextMenu={(e) => handleContextMenu(e, entry)}
                >
                  <td className="px-3 py-2">
                    <div className="flex items-center gap-2">
                      {getFileIcon(entry)}
                      <span className={entry.is_dir ? 'font-medium' : ''}>{entry.name}</span>
                      {!entry.is_dir && isEditable(entry) && (
                        <Edit3 size={12} className="text-zinc-600" title="Editable" />
                      )}
                    </div>
                  </td>
                  <td className="px-3 py-2 text-zinc-500">
                    {entry.is_dir ? '—' : formatSize(entry.size)}
                  </td>
                  <td className="px-3 py-2 text-zinc-500">
                    {formatDate(entry.modified)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
      
      {/* Status Bar */}
      <div className="flex items-center justify-between px-3 py-1.5 bg-zinc-800 border-t border-zinc-700 text-xs text-zinc-500">
        <span className="flex items-center gap-2">
          <span className="flex items-center gap-1">
            <span className="w-1.5 h-1.5 rounded-full bg-green-500 animate-pulse" />
            Live
          </span>
          <span className="text-zinc-600">•</span>
          {contents?.entries.length || 0} items
          {selectedItems.size > 0 && ` • ${selectedItems.size} selected`}
        </span>
        <span className="truncate max-w-xs" title={currentPath}>{currentPath}</span>
      </div>
      
      {/* Context Menu */}
      {contextMenu && (
        <div
          className="fixed bg-zinc-800 border border-zinc-700 rounded-lg shadow-xl py-1 z-50 min-w-48"
          style={{ left: contextMenu.x, top: contextMenu.y }}
          onClick={(e) => e.stopPropagation()}
        >
          {contextMenu.entry ? (
            <>
              <button
                onClick={() => {
                  handleOpen(contextMenu.entry!);
                  setContextMenu(null);
                }}
                className="w-full px-3 py-1.5 text-left hover:bg-zinc-700 flex items-center gap-2"
              >
                {contextMenu.entry.is_dir ? <FolderOpen size={14} /> : <Edit3 size={14} />}
                {contextMenu.entry.is_dir ? 'Open' : (isEditable(contextMenu.entry) ? 'Edit' : 'Open')}
              </button>
              <div className="border-t border-zinc-700 my-1" />
              <button
                onClick={() => {
                  setSelectedItems(new Set([contextMenu.entry!.path]));
                  handleCopy(false);
                }}
                className="w-full px-3 py-1.5 text-left hover:bg-zinc-700 flex items-center gap-2"
              >
                <Copy size={14} /> Copy
              </button>
              <button
                onClick={() => {
                  setSelectedItems(new Set([contextMenu.entry!.path]));
                  handleCopy(true);
                }}
                className="w-full px-3 py-1.5 text-left hover:bg-zinc-700 flex items-center gap-2"
              >
                <Scissors size={14} /> Cut
              </button>
              <div className="border-t border-zinc-700 my-1" />
              <button
                onClick={() => {
                  setRenameDialog({ entry: contextMenu.entry!, newName: contextMenu.entry!.name });
                  setContextMenu(null);
                }}
                className="w-full px-3 py-1.5 text-left hover:bg-zinc-700 flex items-center gap-2"
              >
                <Edit3 size={14} /> Rename
              </button>
              <button
                onClick={() => {
                  setDeleteConfirm([contextMenu.entry!]);
                  setContextMenu(null);
                }}
                className="w-full px-3 py-1.5 text-left hover:bg-zinc-700 flex items-center gap-2 text-red-400"
              >
                <Trash2 size={14} /> Delete
              </button>
            </>
          ) : (
            <>
              <button
                onClick={() => {
                  setNewItemDialog({ type: 'file', name: '' });
                  setContextMenu(null);
                }}
                className="w-full px-3 py-1.5 text-left hover:bg-zinc-700 flex items-center gap-2"
              >
                <FilePlus size={14} /> New File
              </button>
              <button
                onClick={() => {
                  setNewItemDialog({ type: 'folder', name: '' });
                  setContextMenu(null);
                }}
                className="w-full px-3 py-1.5 text-left hover:bg-zinc-700 flex items-center gap-2"
              >
                <FolderPlus size={14} /> New Folder
              </button>
              {clipboard && (
                <>
                  <div className="border-t border-zinc-700 my-1" />
                  <button
                    onClick={handlePaste}
                    className="w-full px-3 py-1.5 text-left hover:bg-zinc-700 flex items-center gap-2"
                  >
                    <Check size={14} /> Paste ({clipboard.paths.length} items)
                  </button>
                </>
              )}
              <div className="border-t border-zinc-700 my-1" />
              <button
                onClick={() => {
                  loadDirectory(currentPath);
                  setContextMenu(null);
                }}
                className="w-full px-3 py-1.5 text-left hover:bg-zinc-700 flex items-center gap-2"
              >
                <RefreshCw size={14} /> Refresh
              </button>
            </>
          )}
        </div>
      )}
      
      {/* Rename Dialog */}
      {renameDialog && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
          <div className="bg-zinc-900 border border-zinc-700 rounded-lg p-4 w-96">
            <h3 className="text-lg font-semibold mb-4">Rename</h3>
            <input
              type="text"
              value={renameDialog.newName}
              onChange={(e) => setRenameDialog({ ...renameDialog, newName: e.target.value })}
              onKeyDown={(e) => e.key === 'Enter' && handleRename()}
              className="input w-full mb-4"
              autoFocus
            />
            <div className="flex justify-end gap-2">
              <button onClick={() => setRenameDialog(null)} className="btn btn-secondary">
                Cancel
              </button>
              <button 
                onClick={handleRename} 
                disabled={!renameDialog.newName || renameDialog.newName === renameDialog.entry.name}
                className="btn btn-primary"
              >
                Rename
              </button>
            </div>
          </div>
        </div>
      )}
      
      {/* New Item Dialog */}
      {newItemDialog && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
          <div className="bg-zinc-900 border border-zinc-700 rounded-lg p-4 w-96">
            <h3 className="text-lg font-semibold mb-4">
              New {newItemDialog.type === 'folder' ? 'Folder' : 'File'}
            </h3>
            <input
              type="text"
              value={newItemDialog.name}
              onChange={(e) => setNewItemDialog({ ...newItemDialog, name: e.target.value })}
              onKeyDown={(e) => e.key === 'Enter' && handleCreateNew()}
              placeholder={newItemDialog.type === 'folder' ? 'Folder name' : 'filename.txt'}
              className="input w-full mb-4"
              autoFocus
            />
            <div className="flex justify-end gap-2">
              <button onClick={() => setNewItemDialog(null)} className="btn btn-secondary">
                Cancel
              </button>
              <button 
                onClick={handleCreateNew} 
                disabled={!newItemDialog.name}
                className="btn btn-primary"
              >
                Create
              </button>
            </div>
          </div>
        </div>
      )}
      
      {/* Delete Confirmation */}
      {deleteConfirm && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
          <div className="bg-zinc-900 border border-zinc-700 rounded-lg p-4 w-96">
            <h3 className="text-lg font-semibold mb-2 text-red-400">Delete</h3>
            <p className="text-zinc-400 mb-4">
              Are you sure you want to delete {deleteConfirm.length === 1 
                ? `"${deleteConfirm[0].name}"` 
                : `${deleteConfirm.length} items`}?
              {deleteConfirm.some(e => e.is_dir) && (
                <span className="block mt-2 text-yellow-400 text-sm">
                  ⚠️ This will delete all contents inside folders
                </span>
              )}
            </p>
            <div className="flex justify-end gap-2">
              <button onClick={() => setDeleteConfirm(null)} className="btn btn-secondary">
                Cancel
              </button>
              <button onClick={handleDelete} className="btn btn-danger">
                Delete
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
