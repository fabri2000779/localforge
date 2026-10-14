//! Host-side metrics history: one append-only JSONL file per server under
//! `<data_root>/metrics/`, sampled every minute, with 14 days queryable, never synced.
//! Expired samples are compacted in daily batches to avoid rewriting two weeks of
//! history every minute. An actively sampled file holds at most 15 days of history.
//! Plain files rather than SQLite: the data is tiny, there is a single writer, and a
//! torn final line is simply skipped on read.

use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use localforge_core::backend::NodeBackend;
use localforge_core::types::{MetricPoint, ServerStatus};

const RETENTION_MS: i64 = 14 * 24 * 60 * 60 * 1000; // 14 days
const COMPACTION_INTERVAL_MS: i64 = 24 * 60 * 60 * 1000; // up to one extra day on disk
const SAMPLE_SECS: u64 = 60;

fn metrics_dir(data_root: &Path) -> PathBuf {
    data_root.join("metrics")
}

/// Per-server file; the id is sanitized so it can't escape the metrics directory.
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

fn temp_for(path: &Path) -> PathBuf {
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    PathBuf::from(tmp)
}

/// Parse a JSONL file, skipping unparseable lines; points with `ts >= since`, oldest first.
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
    // Usually already ordered, but clock corrections and legacy files can contain
    // out-of-order samples. Keep the API's chronological contract in those cases too.
    out.sort_by_key(|p| p.ts);
    out
}

fn retained_points(path: &Path, since_ms: i64, now_ms: i64) -> Vec<MetricPoint> {
    read_points(path, since_ms.max(now_ms.saturating_sub(RETENTION_MS)))
}

/// Metrics since `since_ms` within the last 14 days, oldest first.
/// Query retention is independent of physical compaction, including stopped servers.
pub async fn query_range(
    data_root: &Path,
    server_id: &str,
    since_ms: i64,
) -> Result<Vec<MetricPoint>, String> {
    let path = file_for(data_root, server_id);
    tokio::task::spawn_blocking(move || {
        retained_points(&path, since_ms, Utc::now().timestamp_millis())
    })
    .await
    .map_err(|e| e.to_string())
}

/// Delete a server's metrics file (on server delete).
pub fn remove_server(data_root: &Path, server_id: &str) {
    let path = file_for(data_root, server_id);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(temp_for(&path));
}

static SAMPLER_STARTED: AtomicBool = AtomicBool::new(false);

/// Start the per-process sampler (idempotent).
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
            if let Err(e) = append_and_prune(&data_root, sid, p, cutoff) {
                tracing::warn!(server_id = sid, error = %e, "could not persist metrics sample");
            }
        }
    })
    .await;
}

/// Append one sample; compact when the oldest valid line is a day past the cutoff.
/// The sampler is the single writer and awaits every batch. Deriving the next
/// compaction from the file itself also preserves the cadence across restarts.
/// Returns the bytes rewritten when a compaction happened, for I/O regression tests.
fn append_and_prune(
    data_root: &Path,
    server_id: &str,
    p: &MetricPoint,
    cutoff: i64,
) -> std::io::Result<Option<u64>> {
    let path = file_for(data_root, server_id);
    let line = serde_json::to_string(p).map_err(std::io::Error::other)?;
    {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        writeln!(f, "{line}")?;
    }

    // Usually reads only the first line. A malformed legacy prefix must not
    // prevent retention forever, so skip invalid lines until a sample is found.
    let mut needs_prune = false;
    for line in BufReader::new(std::fs::File::open(&path)?).lines() {
        let line = line?;
        if let Ok(first) = serde_json::from_str::<MetricPoint>(&line) {
            needs_prune = first.ts <= cutoff.saturating_sub(COMPACTION_INTERVAL_MS);
            break;
        }
    }
    if !needs_prune {
        return Ok(None);
    }

    // Stream the retained lines into a temporary file. Never replace the history
    // if reading or writing fails, and close both handles before the Windows rename.
    let tmp = temp_for(&path);
    let result = (|| {
        let input = BufReader::new(std::fs::File::open(&path)?);
        let mut output = BufWriter::new(std::fs::File::create(&tmp)?);
        let mut bytes = 0;
        for line in input.lines() {
            let line = line?;
            if let Ok(point) = serde_json::from_str::<MetricPoint>(&line) {
                if point.ts >= cutoff {
                    output.write_all(line.as_bytes())?;
                    output.write_all(b"\n")?;
                    bytes += line.len() as u64 + 1;
                }
            }
        }
        output.flush()?;
        drop(output);
        std::fs::rename(&tmp, &path)?;
        Ok(Some(bytes))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE_MS: i64 = SAMPLE_SECS as i64 * 1000;

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("localforge-metrics-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(metrics_dir(&path)).unwrap();
            Self(path)
        }

        fn seed(&self, id: &str, samples: impl IntoIterator<Item = MetricPoint>) {
            let mut file = BufWriter::new(std::fs::File::create(file_for(&self.0, id)).unwrap());
            for point in samples {
                writeln!(file, "{}", serde_json::to_string(&point).unwrap()).unwrap();
            }
            file.flush().unwrap();
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn point(ts: i64) -> MetricPoint {
        MetricPoint {
            ts,
            cpu_percent: 12.5,
            memory_mb: 512.0,
            net_rx_bytes: 1234,
            net_tx_bytes: 5678,
        }
    }

    #[test]
    fn a_day_of_samples_compacts_two_weeks_of_history_only_once() {
        let root = TestRoot::new();
        let path = file_for(&root.0, "server");
        let initial_minutes = RETENTION_MS / MINUTE_MS;
        root.seed(
            "server",
            (0..=initial_minutes).map(|minute| point(minute * MINUTE_MS)),
        );
        let mut compactions = 0;
        let mut compacted_bytes = 0;
        let mut appended_bytes = 0;
        let mut former_rewrite_bytes = 0;
        // Old pruning would rewrite the full 20,161-point window after every one
        // of these 1,440 appends. Count equivalent bytes without doing that I/O.
        let mut window_bytes = std::fs::metadata(&path).unwrap().len();
        for minute in 1..=1440 {
            let now = RETENTION_MS + minute * MINUTE_MS;
            let sample = point(now);
            let sample_bytes = serde_json::to_vec(&sample).unwrap().len() as u64 + 1;
            appended_bytes += sample_bytes;
            window_bytes += sample_bytes;
            window_bytes -= serde_json::to_vec(&point((minute - 1) * MINUTE_MS))
                .unwrap()
                .len() as u64
                + 1;
            former_rewrite_bytes += window_bytes;
            if let Some(bytes) =
                append_and_prune(&root.0, "server", &sample, now - RETENTION_MS).unwrap()
            {
                compactions += 1;
                compacted_bytes += bytes;
            }
            if minute == 1 || minute == 1439 || minute == 1440 {
                let retained = retained_points(&path, i64::MIN, now);
                assert_eq!(retained.len() as i64, initial_minutes + 1);
                assert_eq!(retained.first().unwrap().ts, now - RETENTION_MS);
                assert_eq!(retained.last().unwrap().ts, now);
                assert!(retained.windows(2).all(|pair| pair[0].ts < pair[1].ts));
            }
        }
        assert_eq!(compactions, 1);
        assert_eq!(compacted_bytes, std::fs::metadata(&path).unwrap().len());
        assert!((appended_bytes + compacted_bytes) * 100 < appended_bytes + former_rewrite_bytes);
        println!("daily metrics writes: {appended_bytes} appended + {compacted_bytes} compacted; previous: {appended_bytes} appended + {former_rewrite_bytes} compacted");

        // No in-memory cadence to lose after a restart: the next sample must
        // append normally, although the oldest retained point is now expired.
        let now = RETENTION_MS + COMPACTION_INTERVAL_MS + MINUTE_MS;
        assert_eq!(
            append_and_prune(&root.0, "server", &point(now), now - RETENTION_MS).unwrap(),
            None
        );
        let retained = retained_points(&path, now - 2 * MINUTE_MS, now);
        assert_eq!(
            retained.iter().map(|p| p.ts).collect::<Vec<_>>(),
            vec![now - 2 * MINUTE_MS, now - MINUTE_MS, now]
        );
    }

    #[test]
    fn legacy_files_and_stopped_servers_obey_query_cutoff_and_order() {
        let root = TestRoot::new();
        let now = RETENTION_MS + 1000;
        root.seed("legacy", [point(now), point(999), point(1000), point(1001)]);
        let path = file_for(&root.0, "legacy");
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(file, "{{torn line").unwrap();
        drop(file);
        assert_eq!(
            retained_points(&path, i64::MIN, now)
                .iter()
                .map(|p| p.ts)
                .collect::<Vec<_>>(),
            vec![1000, 1001, now]
        );
        assert_eq!(
            retained_points(&path, 1001, now)
                .iter()
                .map(|p| p.ts)
                .collect::<Vec<_>>(),
            vec![1001, now]
        );
        assert!(retained_points(&path, i64::MIN, now + RETENTION_MS + 1).is_empty());
    }

    #[test]
    fn malformed_prefix_does_not_disable_compaction_and_delete_removes_temp() {
        let root = TestRoot::new();
        let path = file_for(&root.0, "server");
        std::fs::write(
            &path,
            format!("\n{{bad\n{}\n", serde_json::to_string(&point(0)).unwrap()),
        )
        .unwrap();
        let now = RETENTION_MS + COMPACTION_INTERVAL_MS;
        assert!(
            append_and_prune(&root.0, "server", &point(now), now - RETENTION_MS)
                .unwrap()
                .is_some()
        );
        assert_eq!(read_points(&path, i64::MIN).len(), 1);
        assert!(!temp_for(&path).exists());
        std::fs::write(temp_for(&path), "interrupted compaction").unwrap();
        remove_server(&root.0, "server");
        assert!(!path.exists());
        assert!(!temp_for(&path).exists());
    }

    #[test]
    fn failed_compaction_preserves_original_and_retries_next_sample() {
        let root = TestRoot::new();
        root.seed("server", [point(0)]);
        let path = file_for(&root.0, "server");
        let tmp = temp_for(&path);
        // An uncreatable temp path simulates a write failure without platform-
        // dependent permissions. The append remains available in the original.
        std::fs::create_dir(&tmp).unwrap();
        let now = RETENTION_MS + COMPACTION_INTERVAL_MS;
        assert!(append_and_prune(&root.0, "server", &point(now), now - RETENTION_MS).is_err());
        assert_eq!(read_points(&path, i64::MIN).len(), 2);
        std::fs::remove_dir(&tmp).unwrap();
        let next = now + MINUTE_MS;
        assert!(
            append_and_prune(&root.0, "server", &point(next), next - RETENTION_MS)
                .unwrap()
                .is_some()
        );
        let retained = read_points(&path, i64::MIN);
        assert_eq!(
            retained.iter().map(|p| p.ts).collect::<Vec<_>>(),
            vec![now, next]
        );
        assert!(!tmp.exists());
    }
}
