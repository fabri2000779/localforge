//! Bring-your-own-S3 backups.
//!
//! Archive a server's data dir (tar + gzip) and ship it to the user's own
//! S3-compatible bucket; list / restore / delete. Runs ON THE HOST, so both
//! the desktop and the headless agent get it for free (it's part of
//! `LocalDockerBackend`). No OpenSSL — `rust-s3` with the rustls feature, matching
//! the workspace's single TLS stack.
//!
//! v1 archives the data dir as-is (a live snapshot). For a perfectly consistent
//! backup the UI recommends stopping the server first; a Minecraft `save-off` /
//! `save-all flush` / `save-on` fence is a documented follow-up. Restore always
//! stops the server and moves the current data aside before extracting, so a
//! failed restore never destroys the existing world.

use std::path::{Path, PathBuf};

use localforge_core::backend::{BackendError, Result};
use localforge_core::types::{BackupEntry, BackupTarget};
use s3::creds::Credentials;
use s3::{Bucket, Region};
use tokio::io::AsyncWriteExt;

/// Object-key prefix for a server's backups within the bucket.
fn server_prefix(server_id: &str) -> String {
    format!("localforge/{server_id}/")
}

fn s3_err<E: std::fmt::Display>(e: E) -> BackendError {
    BackendError::Other(format!("s3: {e}"))
}

/// Build an S3 client for the user's destination. Path-style for MinIO and many
/// S3-compatible providers; virtual-hosted (the default) for AWS.
fn bucket_for(target: &BackupTarget) -> Result<Bucket> {
    let region = Region::Custom {
        region: target.region.clone(),
        endpoint: target.endpoint.clone(),
    };
    let creds = Credentials::new(
        Some(&target.access_key),
        Some(&target.secret_key),
        None,
        None,
        None,
    )
    .map_err(s3_err)?;
    let bucket = Bucket::new(&target.bucket, region, creds).map_err(s3_err)?;
    let bucket = if target.path_style {
        bucket.with_path_style()
    } else {
        bucket
    };
    Ok(*bucket)
}

// ---------------------------------------------------------------------------
// Archive helpers (blocking; run via spawn_blocking)
// ---------------------------------------------------------------------------

fn archive_dir(src_dir: &Path) -> Result<PathBuf> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    let tmp = std::env::temp_dir().join(format!(
        "localforge-backup-{}.tar.gz",
        uuid::Uuid::new_v4()
    ));
    let file = std::fs::File::create(&tmp).map_err(BackendError::io)?;
    let enc = GzEncoder::new(file, Compression::default());
    let mut builder = tar::Builder::new(enc);
    // Archive the dir CONTENTS at the archive root, so extraction lands the
    // files directly back into the (recreated) data dir.
    builder
        .append_dir_all(".", src_dir)
        .map_err(BackendError::io)?;
    let enc = builder.into_inner().map_err(BackendError::io)?;
    enc.finish().map_err(BackendError::io)?;
    Ok(tmp)
}

fn extract_into(archive: &Path, dest_dir: &Path) -> Result<()> {
    use flate2::read::GzDecoder;
    std::fs::create_dir_all(dest_dir).map_err(BackendError::io)?;
    let file = std::fs::File::open(archive).map_err(BackendError::io)?;
    let mut ar = tar::Archive::new(GzDecoder::new(file));
    ar.unpack(dest_dir).map_err(BackendError::io)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Public ops (called from the NodeBackend impl)
// ---------------------------------------------------------------------------

/// Archive `data_path` and stream it to the bucket under the server's prefix.
/// Returns the object key written.
pub async fn upload(target: &BackupTarget, server_id: &str, data_path: &Path) -> Result<String> {
    let src = data_path.to_path_buf();
    let tmp = tokio::task::spawn_blocking(move || archive_dir(&src))
        .await
        .map_err(BackendError::other)??;

    let stamp = chrono::Utc::now().format("%Y-%m-%dT%H-%M-%SZ").to_string();
    let key = format!("{}{}.tar.gz", server_prefix(server_id), stamp);

    let bucket = bucket_for(target)?;
    let mut reader = tokio::fs::File::open(&tmp).await.map_err(BackendError::io)?;
    let put = bucket.put_object_stream(&mut reader, &key).await;
    let _ = tokio::fs::remove_file(&tmp).await; // best-effort cleanup
    put.map_err(s3_err)?;
    Ok(key)
}

/// List the server's backup objects in the bucket, newest first.
pub async fn list(target: &BackupTarget, server_id: &str) -> Result<Vec<BackupEntry>> {
    let bucket = bucket_for(target)?;
    let pages = bucket
        .list(server_prefix(server_id), None)
        .await
        .map_err(s3_err)?;
    let mut out = Vec::new();
    for page in pages {
        for obj in page.contents {
            let created_at = chrono::DateTime::parse_from_rfc3339(&obj.last_modified)
                .map(|d| d.timestamp_millis())
                .unwrap_or(0);
            out.push(BackupEntry {
                key: obj.key,
                size: obj.size,
                created_at,
            });
        }
    }
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(out)
}

/// Download `key` and extract it over `dest_dir`. The caller has already
/// stopped the server and moved the old data aside.
pub async fn download_extract(target: &BackupTarget, key: &str, dest_dir: &Path) -> Result<()> {
    let bucket = bucket_for(target)?;
    let tmp = std::env::temp_dir().join(format!(
        "localforge-restore-{}.tar.gz",
        uuid::Uuid::new_v4()
    ));
    {
        let mut out = tokio::fs::File::create(&tmp).await.map_err(BackendError::io)?;
        // Stream S3 → file so a multi-GB world isn't buffered in RAM (the
        // upload side streams too, via put_object_stream).
        bucket
            .get_object_to_writer(key, &mut out)
            .await
            .map_err(s3_err)?;
        out.flush().await.map_err(BackendError::io)?;
    }
    let archive = tmp.clone();
    let dest = dest_dir.to_path_buf();
    let res = tokio::task::spawn_blocking(move || extract_into(&archive, &dest))
        .await
        .map_err(BackendError::other)?;
    let _ = tokio::fs::remove_file(&tmp).await;
    res
}

/// Delete a backup object from the bucket.
pub async fn delete(target: &BackupTarget, key: &str) -> Result<()> {
    let bucket = bucket_for(target)?;
    bucket.delete_object(key).await.map_err(s3_err)?;
    Ok(())
}
