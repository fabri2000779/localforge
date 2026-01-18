// File manager commands for browsing, editing, and managing server files

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<u64>, // Unix timestamp
    pub extension: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryContents {
    pub path: String,
    pub parent: Option<String>,
    pub entries: Vec<FileEntry>,
}

/// List contents of a directory
#[tauri::command]
pub async fn list_directory(path: String) -> Result<DirectoryContents, String> {
    let dir_path = PathBuf::from(&path);
    
    if !dir_path.exists() {
        return Err(format!("Directory does not exist: {}", path));
    }
    
    if !dir_path.is_dir() {
        return Err(format!("Path is not a directory: {}", path));
    }
    
    let mut entries = Vec::new();
    
    let read_dir = fs::read_dir(&dir_path).map_err(|e| e.to_string())?;
    
    for entry in read_dir {
        let entry = entry.map_err(|e| e.to_string())?;
        let metadata = entry.metadata().map_err(|e| e.to_string())?;
        let file_name = entry.file_name().to_string_lossy().to_string();
        
        // Skip hidden files (starting with .)
        if file_name.starts_with('.') {
            continue;
        }
        
        let modified = metadata.modified().ok().and_then(|t| {
            t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs())
        });
        
        let extension = if metadata.is_file() {
            Path::new(&file_name)
                .extension()
                .map(|e| e.to_string_lossy().to_string())
        } else {
            None
        };
        
        entries.push(FileEntry {
            name: file_name,
            path: entry.path().to_string_lossy().to_string(),
            is_dir: metadata.is_dir(),
            size: metadata.len(),
            modified,
            extension,
        });
    }
    
    // Sort: directories first, then by name
    entries.sort_by(|a, b| {
        match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        }
    });
    
    let parent = dir_path.parent().map(|p| p.to_string_lossy().to_string());
    
    Ok(DirectoryContents {
        path,
        parent,
        entries,
    })
}

/// Read file contents as text
#[tauri::command]
pub async fn read_file_text(path: String) -> Result<String, String> {
    let file_path = PathBuf::from(&path);
    
    if !file_path.exists() {
        return Err(format!("File does not exist: {}", path));
    }
    
    if !file_path.is_file() {
        return Err(format!("Path is not a file: {}", path));
    }
    
    // Check file size (limit to 5MB for text editing)
    let metadata = fs::metadata(&file_path).map_err(|e| e.to_string())?;
    if metadata.len() > 5 * 1024 * 1024 {
        return Err("File is too large to edit (max 5MB)".to_string());
    }
    
    fs::read_to_string(&file_path).map_err(|e| format!("Failed to read file: {}", e))
}

/// Write text content to a file
#[tauri::command]
pub async fn write_file_text(path: String, content: String) -> Result<(), String> {
    let file_path = PathBuf::from(&path);
    
    // Ensure parent directory exists
    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    
    fs::write(&file_path, content).map_err(|e| format!("Failed to write file: {}", e))
}

/// Create a new file
#[tauri::command]
pub async fn create_file(path: String, content: Option<String>) -> Result<(), String> {
    let file_path = PathBuf::from(&path);
    
    if file_path.exists() {
        return Err(format!("File already exists: {}", path));
    }
    
    // Ensure parent directory exists
    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    
    fs::write(&file_path, content.unwrap_or_default())
        .map_err(|e| format!("Failed to create file: {}", e))
}

/// Create a new directory
#[tauri::command]
pub async fn create_directory(path: String) -> Result<(), String> {
    let dir_path = PathBuf::from(&path);
    
    if dir_path.exists() {
        return Err(format!("Directory already exists: {}", path));
    }
    
    fs::create_dir_all(&dir_path).map_err(|e| format!("Failed to create directory: {}", e))
}

/// Delete a file or directory
#[tauri::command]
pub async fn delete_path(path: String) -> Result<(), String> {
    let target_path = PathBuf::from(&path);
    
    if !target_path.exists() {
        return Err(format!("Path does not exist: {}", path));
    }
    
    if target_path.is_dir() {
        fs::remove_dir_all(&target_path).map_err(|e| format!("Failed to delete directory: {}", e))
    } else {
        fs::remove_file(&target_path).map_err(|e| format!("Failed to delete file: {}", e))
    }
}

/// Rename a file or directory
#[tauri::command]
pub async fn rename_path(old_path: String, new_name: String) -> Result<String, String> {
    let old = PathBuf::from(&old_path);
    
    if !old.exists() {
        return Err(format!("Path does not exist: {}", old_path));
    }
    
    // Validate new name (no path separators allowed)
    if new_name.contains('/') || new_name.contains('\\') {
        return Err("Invalid name: cannot contain path separators".to_string());
    }
    
    let new = old.parent()
        .ok_or("Cannot rename root")?
        .join(&new_name);
    
    if new.exists() {
        return Err(format!("A file or folder with that name already exists: {}", new_name));
    }
    
    fs::rename(&old, &new).map_err(|e| format!("Failed to rename: {}", e))?;
    
    Ok(new.to_string_lossy().to_string())
}

/// Move a file or directory to a new location
#[tauri::command]
pub async fn move_path(source: String, destination_dir: String) -> Result<String, String> {
    let src = PathBuf::from(&source);
    let dest_dir = PathBuf::from(&destination_dir);
    
    if !src.exists() {
        return Err(format!("Source does not exist: {}", source));
    }
    
    if !dest_dir.is_dir() {
        return Err(format!("Destination is not a directory: {}", destination_dir));
    }
    
    let file_name = src.file_name()
        .ok_or("Invalid source path")?;
    
    let dest = dest_dir.join(file_name);
    
    if dest.exists() {
        return Err(format!("Destination already exists: {}", dest.display()));
    }
    
    fs::rename(&src, &dest).map_err(|e| format!("Failed to move: {}", e))?;
    
    Ok(dest.to_string_lossy().to_string())
}

/// Copy a file or directory
#[tauri::command]
pub async fn copy_path(source: String, destination_dir: String) -> Result<String, String> {
    let src = PathBuf::from(&source);
    let dest_dir = PathBuf::from(&destination_dir);
    
    if !src.exists() {
        return Err(format!("Source does not exist: {}", source));
    }
    
    if !dest_dir.is_dir() {
        return Err(format!("Destination is not a directory: {}", destination_dir));
    }
    
    let file_name = src.file_name()
        .ok_or("Invalid source path")?;
    
    let dest = dest_dir.join(file_name);
    
    if dest.exists() {
        return Err(format!("Destination already exists: {}", dest.display()));
    }
    
    if src.is_dir() {
        copy_dir_recursive(&src, &dest)?;
    } else {
        fs::copy(&src, &dest).map_err(|e| format!("Failed to copy: {}", e))?;
    }
    
    Ok(dest.to_string_lossy().to_string())
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<(), String> {
    fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    
    for entry in fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let entry_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        
        if entry_path.is_dir() {
            copy_dir_recursive(&entry_path, &dest_path)?;
        } else {
            fs::copy(&entry_path, &dest_path).map_err(|e| e.to_string())?;
        }
    }
    
    Ok(())
}

/// Get file info
#[tauri::command]
pub async fn get_file_info(path: String) -> Result<FileEntry, String> {
    let file_path = PathBuf::from(&path);
    
    if !file_path.exists() {
        return Err(format!("Path does not exist: {}", path));
    }
    
    let metadata = fs::metadata(&file_path).map_err(|e| e.to_string())?;
    let file_name = file_path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    
    let modified = metadata.modified().ok().and_then(|t| {
        t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs())
    });
    
    let extension = if metadata.is_file() {
        file_path.extension().map(|e| e.to_string_lossy().to_string())
    } else {
        None
    };
    
    Ok(FileEntry {
        name: file_name,
        path,
        is_dir: metadata.is_dir(),
        size: metadata.len(),
        modified,
        extension,
    })
}
