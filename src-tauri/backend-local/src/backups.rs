//! Bring-your-own-S3 backups (tar+gzip via rust-s3/rustls), run on the host by `LocalDockerBackend`.

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

/// Keys arrive from clients (on the agent, possibly a lower-privileged one): confine every
/// op to `localforge/<id>/` so no other object in the bucket can be read or deleted.
fn ensure_key_in_prefix(server_id: &str, key: &str) -> Result<()> {
    let prefix = server_prefix(server_id);
    if key.starts_with(&prefix) {
        Ok(())
    } else {
        Err(BackendError::invalid(
            "backup key is outside this server's backup prefix",
        ))
    }
}

fn s3_err<E: std::fmt::Display>(e: E) -> BackendError {
    BackendError::Other(format!("s3: {e}"))
}

/// S3 client for the target; path-style for MinIO-like providers, virtual-hosted for AWS.
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

// Archive helpers (blocking; run via spawn_blocking).

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
    // Archive the dir CONTENTS at the root so extraction lands directly in the data dir.
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

/// Archive `data_path` and upload it under the server's prefix; returns the object key.
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
    let _ = tokio::fs::remove_file(&tmp).await;
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
    out.sort_by_key(|b| std::cmp::Reverse(b.created_at));
    Ok(out)
}

/// Download `key` to a temp file (validating the prefix first) so a restore can swap
/// directories only once the bytes are on disk. The caller deletes the file.
pub async fn download_archive(
    target: &BackupTarget,
    server_id: &str,
    key: &str,
) -> Result<PathBuf> {
    ensure_key_in_prefix(server_id, key)?;
    let bucket = bucket_for(target)?;
    let tmp = std::env::temp_dir().join(format!(
        "localforge-restore-{}.tar.gz",
        uuid::Uuid::new_v4()
    ));
    let res = async {
        let mut out = tokio::fs::File::create(&tmp).await.map_err(BackendError::io)?;
        // Stream to disk so a multi-GB world isn't buffered in RAM.
        bucket
            .get_object_to_writer(key, &mut out)
            .await
            .map_err(s3_err)?;
        out.flush().await.map_err(BackendError::io)
    }
    .await;
    if let Err(e) = res {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(e);
    }
    Ok(tmp)
}

/// Extract a downloaded archive into `dest_dir` on a blocking thread; the archive is kept.
pub async fn extract_archive(archive: &Path, dest_dir: &Path) -> Result<()> {
    let archive = archive.to_path_buf();
    let dest = dest_dir.to_path_buf();
    tokio::task::spawn_blocking(move || extract_into(&archive, &dest))
        .await
        .map_err(BackendError::other)?
}

/// Delete a backup object from the bucket.
pub async fn delete(target: &BackupTarget, server_id: &str, key: &str) -> Result<()> {
    ensure_key_in_prefix(server_id, key)?;
    let bucket = bucket_for(target)?;
    bucket.delete_object(key).await.map_err(s3_err)?;
    Ok(())
}
