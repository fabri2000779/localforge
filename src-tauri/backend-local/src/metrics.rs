//! Local metrics history (CPU / RAM / network time-series).
//!
//! Stored ON THE HOST as one append-only JSON-lines file per server under
//! `<data_root>/metrics/<server_id>.jsonl`, and **never synced to the cloud** —
//! viewed by querying the host on demand, exactly like logs and live stats. A
//! single per-process sampler loop polls each running server's stats every
//! minute, appends one line, and prunes anything past the retention window.
//!
//! Why files and not SQLite: Docker only streams the *live* stat — it keeps no
//! history — so something has to persist the samples for the graphs. At this
//! cadence that data is tiny (one line per server per minute → ~20k lines over
//! the 14-day window), so a linear scan is sub-millisecond and there is no need
//! for a SQL engine. Plain JSONL keeps the crate pure-Rust with zero native
//! deps — consistent with the rest of the codebase (rustls over OpenSSL,
//! pure-Rust tar/flate2) — is trivially inspectable, and needs no migrations.
//! Only the sampler writes, so there are no concurrent-writer races; a torn
//! final line from a crash is simply skipped on read.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use localforge_core::backend::NodeBackend;
use localforge_core::types::{MetricPoint, ServerStatus};

const RETENTION_MS: i64 = 14 * 24 * 60 * 60 * 1000; // 14 days
const SAMPLE_SECS: u64 = 60;

fn metrics_dir(data_root: &Path) -> PathBuf {
    data_root.join("metrics")
}

/// Per-server time-series file. The id is sanitized so it can never escape the
/// metrics directory (ids are uuids in practice, but be defensive).
fn file_for(data_root: &Path, server_id: &str) -> PathBuf {
    let safe: String = server_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    metrics_dir(data_root).join(format!("{safe}.jsonl"))
}

/// Parse a JSONL metrics file, skipping any unparseable (e.g. torn final)
/// lines, returning points with `ts >= since` in file order (oldest first).
fn read_points(path: &Path, since: i64) -> Vec<MetricPoint> {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(p) = serde_json::from_str::<MetricPoint>(line) {
            if p.ts >= since {
                out.push(p);
            }
        }
    }
    out
}

/// Read a server's metrics since `since_ms` (oldest first). Runs the blocking
/// file read on a blocking thread to avoid stalling the async runtime.
pub async fn query_range(
    data_root: &Path,
    server_id: &str,
    since_ms: i64,
) -> Result<Vec<MetricPoint>, String> {
    let path = file_for(data_root, server_id);
    tokio::task::spawn_blocking(move || read_points(&path, since_ms))
        .await
        .map_err(|e| e.to_string())
}

/// Delete a server's metrics file. Called when the server is deleted so its
/// history doesn't linger as dead space on disk.
pub fn remove_server(data_root: &Path, server_id: &str) {
    let _ = std::fs::remove_file(file_for(data_root, server_id));
}

static SAMPLER_STARTED: AtomicBool = AtomicBool::new(false);

/// Start the per-process metrics sampler (idempotent). Polls every running
/// server's stats every minute, appends to the local JSONL store, and prunes
/// past the retention window.
pub fn spawn_sampler(backend: Arc<dyn NodeBackend>, data_root: PathBuf) {
    if SAMPLER_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    tokio::spawn(async move {
        // Let startup settle before the first sample.
        tokio::time::sleep(Duration::from_secs(15)).await;
        loop {
            sample_once(&*backend, &data_root).await;
            tokio::time::sleep(Duration::from_secs(SAMPLE_SECS)).await;
        }
    });
}

async fn sample_once(backend: &dyn NodeBackend, data_root: &Path) {
    let servers = match backend.list_servers().await {
        Ok(s) => s,
        Err(_) => return,
    };
    let now = Utc::now().timestamp_millis();
    let mut points: Vec<(String, MetricPoint)> = Vec::new();
    for s in servers {
        if s.status != ServerStatus::Running {
            continue;
        }
        if let Ok(st) = backend.get_stats(&s.id).await {
            points.push((
                s.id.clone(),
                MetricPoint {
                    ts: now,
                    cpu_percent: st.cpu_percent,
                    memory_mb: st.memory_usage_mb,
                    net_rx_bytes: st.net_rx_bytes,
                    net_tx_bytes: st.net_tx_bytes,
                },
            ));
        }
    }
    if points.is_empty() {
        return;
    }
    let data_root = data_root.to_path_buf();
    let cutoff = now - RETENTION_MS;
    let _ = tokio::task::spawn_blocking(move || {
        if std::fs::create_dir_all(metrics_dir(&data_root)).is_err() {
            return;
        }
        for (sid, p) in &points {
            append_and_prune(&data_root, sid, p, cutoff);
        }
    })
    .await;
}

/// Append one sample, then prune the file only if its oldest line has aged past
/// the retention cutoff. The common path is therefore a cheap single-line
/// append; pruning rewrites via a temp file + atomic rename so a crash mid-prune
/// can't lose the whole series.
fn append_and_prune(data_root: &Path, server_id: &str, p: &MetricPoint, cutoff: i64) {
    let path = file_for(data_root, server_id);
    // Append the new sample (one line, single write).
    if let Ok(line) = serde_json::to_string(p) {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = writeln!(f, "{line}");
        }
    }
    // Cheap staleness probe: is the FIRST (oldest) line past the cutoff?
    let needs_prune = match std::fs::File::open(&path) {
        Ok(f) => {
            let mut first = String::new();
            let _ = BufReader::new(f).read_line(&mut first);
            serde_json::from_str::<MetricPoint>(first.trim())
                .map(|fp| fp.ts < cutoff)
                .unwrap_or(false)
        }
        Err(_) => false,
    };
    if !needs_prune {
        return;
    }
    // Rewrite, keeping only points within the window.
    let kept = read_points(&path, cutoff);
    let mut tmp = path.clone().into_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    if let Ok(mut f) = std::fs::File::create(&tmp) {
        let mut buf = String::new();
        for pt in &kept {
            if let Ok(line) = serde_json::to_string(pt) {
                buf.push_str(&line);
                buf.push('\n');
            }
        }
        if f.write_all(buf.as_bytes()).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        } else {
            let _ = std::fs::remove_file(&tmp);
        }
    }
}
