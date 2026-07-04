//! AWS S3 client wrapper.
//!
//! Provides async functions for all S3 operations used by the application.
//! All functions take `&aws_sdk_s3::Client` (cheaply clonable `Arc` inside)
//! and return `anyhow::Result<T>`.

use anyhow::Result;

use crate::types::{EntryKind, S3Entry, S3Location, UploadStorageClass};

// ── Client construction ────────────────────────────────────────────────────────

/// Build the S3 client from the default AWS credential chain.
///
/// Resolves credentials and region with the same precedence as AWS CLI v2:
/// env vars → `~/.aws/credentials` → `~/.aws/config` → EC2 metadata.
///
/// # Errors
///
/// Returns an error if credential resolution fails (e.g. no credentials configured).
pub async fn build_client() -> Result<aws_sdk_s3::Client> {
    let sdk_config = aws_config::load_from_env().await;
    Ok(aws_sdk_s3::Client::new(&sdk_config))
}

// ── StorageClass conversion ────────────────────────────────────────────────────

/// Convert our SDK-agnostic `UploadStorageClass` into the AWS SDK type.
impl From<UploadStorageClass> for aws_sdk_s3::types::StorageClass {
    fn from(sc: UploadStorageClass) -> Self {
        match sc {
            UploadStorageClass::Standard => Self::Standard,
            UploadStorageClass::StandardIa => Self::StandardIa,
            UploadStorageClass::OnezoneIa => Self::OnezoneIa,
            UploadStorageClass::IntelligentTiering => Self::IntelligentTiering,
            UploadStorageClass::Glacier => Self::Glacier,
            UploadStorageClass::GlacierIr => Self::GlacierIr,
            UploadStorageClass::DeepArchive => Self::DeepArchive,
        }
    }
    // aws_sdk_s3::types::StorageClass is #[non_exhaustive].
    // Converting FROM our own exhaustive enum does not require a wildcard arm.
}

// ── Operations ────────────────────────────────────────────────────────────────

/// List all S3 buckets accessible with the current credentials.
///
/// # Errors
///
/// Returns an error if the `ListBuckets` API call fails.
pub async fn list_buckets(client: &aws_sdk_s3::Client) -> Result<Vec<String>> {
    let resp = client.list_buckets().send().await?;
    let names = resp
        .buckets()
        .iter()
        .filter_map(|b| b.name().map(str::to_owned))
        .collect();
    Ok(names)
}

/// List one level of an S3 prefix, returning virtual folders and objects.
///
/// Always uses `.delimiter("/")` so S3 returns common prefixes (virtual
/// sub-folders) separately from real objects at this exact level. Without the
/// delimiter every object under all sub-prefixes would be returned flat.
///
/// Uses the SDK paginator to handle continuation tokens automatically
/// (`list_objects_v2` returns at most 1000 keys per page).
///
/// # Errors
///
/// Returns an error if any page of the `ListObjectsV2` paginator fails.
pub async fn list_prefix(
    client: &aws_sdk_s3::Client,
    location: &S3Location,
) -> Result<Vec<S3Entry>> {
    let mut entries: Vec<S3Entry> = Vec::new();

    let mut paginator = client
        .list_objects_v2()
        .bucket(&location.bucket)
        .prefix(&location.prefix)
        .delimiter("/") // essential for virtual folder behaviour
        .into_paginator()
        .send();

    while let Some(page) = paginator.next().await {
        let page = page?;

        // Common prefixes = virtual sub-folders.
        for cp in page.common_prefixes() {
            if let Some(prefix) = cp.prefix() {
                let name = prefix
                    .trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .unwrap_or(prefix)
                    .to_owned();
                entries.push(S3Entry {
                    key: prefix.to_owned(),
                    name,
                    size_bytes: 0,
                    last_modified: None,
                    e_tag: None,
                    kind: EntryKind::Directory,
                });
            }
        }

        // Contents = real objects at this level.
        for obj in page.contents() {
            let key = obj.key().unwrap_or_default().to_owned();
            let name = key.rsplit('/').next().unwrap_or(&key).to_owned();
            if name.is_empty() {
                continue; // skip the prefix "folder" object itself
            }

            let last_modified = obj.last_modified().and_then(|dt| {
                chrono::DateTime::from_timestamp(dt.secs(), dt.subsec_nanos())
                    .map(|d| d.with_timezone(&chrono::Utc))
            });

            entries.push(S3Entry {
                key,
                name,
                size_bytes: u64::try_from(obj.size().unwrap_or(0)).unwrap_or(0),
                last_modified,
                e_tag: obj.e_tag().map(str::to_owned),
                kind: EntryKind::File,
            });
        }
    }

    // Sort: directories first, then by name case-insensitively.
    entries.sort_by(|a, b| match (&a.kind, &b.kind) {
        (EntryKind::Directory, EntryKind::File) => std::cmp::Ordering::Less,
        (EntryKind::File, EntryKind::Directory) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    Ok(entries)
}

/// Upload a local file to S3, applying the configured storage class.
///
/// # Errors
///
/// Returns an error if the file cannot be read or the `PutObject` call fails.
pub async fn upload_file(
    client: &aws_sdk_s3::Client,
    local_path: &std::path::Path,
    bucket: &str,
    key: &str,
    storage_class: UploadStorageClass,
) -> Result<()> {
    let body = tokio::fs::read(local_path).await?;
    let byte_stream = aws_sdk_s3::primitives::ByteStream::from(body);

    client
        .put_object()
        .bucket(bucket)
        .key(key)
        .body(byte_stream)
        .storage_class(aws_sdk_s3::types::StorageClass::from(storage_class))
        .send()
        .await?;

    Ok(())
}

/// Download an S3 object to a local file, creating parent directories as needed.
///
/// # Errors
///
/// Returns an error if the `GetObject` call fails or the file cannot be written.
pub async fn download_object(
    client: &aws_sdk_s3::Client,
    bucket: &str,
    key: &str,
    local_path: &std::path::Path,
) -> Result<()> {
    use tokio::io::AsyncWriteExt as _;

    let resp = client.get_object().bucket(bucket).key(key).send().await?;

    if let Some(parent) = local_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let bytes = resp.body.collect().await?.into_bytes();
    let mut file = tokio::fs::File::create(local_path).await?;
    file.write_all(&bytes).await?;
    Ok(())
}

/// Delete a single S3 object.
///
/// # Errors
///
/// Returns an error if the `DeleteObject` API call fails.
pub async fn delete_object(client: &aws_sdk_s3::Client, bucket: &str, key: &str) -> Result<()> {
    client
        .delete_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await?;
    Ok(())
}

/// Retrieve size and last-modified for a single S3 object.
///
/// Used by the sync engine for targeted checks, not bulk listings.
///
/// # Errors
///
/// Returns an error if the `HeadObject` API call fails (e.g. object not found).
#[allow(dead_code)]
pub async fn head_object(
    client: &aws_sdk_s3::Client,
    bucket: &str,
    key: &str,
) -> Result<(u64, chrono::DateTime<chrono::Utc>)> {
    let resp = client.head_object().bucket(bucket).key(key).send().await?;
    let size = u64::try_from(resp.content_length().unwrap_or(0)).unwrap_or(0);
    let mtime = resp
        .last_modified()
        .and_then(|dt| {
            chrono::DateTime::from_timestamp(dt.secs(), dt.subsec_nanos())
                .map(|d| d.with_timezone(&chrono::Utc))
        })
        .unwrap_or_default();
    Ok((size, mtime))
}

/// List **all** objects under a prefix recursively (no delimiter).
///
/// Unlike `list_prefix`, this omits the delimiter so every object at every
/// depth under `prefix` is returned as a flat `Vec<S3Entry>`. Common prefixes
/// (virtual folders) are never returned — only real objects.
///
/// Used for recursive folder downloads. Pagination is handled automatically.
///
/// # Errors
///
/// Returns an error if any page of `ListObjectsV2` fails.
pub async fn list_all_objects_recursive(
    client: &aws_sdk_s3::Client,
    bucket: &str,
    prefix: &str,
) -> Result<Vec<S3Entry>> {
    let mut entries: Vec<S3Entry> = Vec::new();

    let mut paginator = client
        .list_objects_v2()
        .bucket(bucket)
        .prefix(prefix)
        // No delimiter — returns every object at every depth.
        .into_paginator()
        .send();

    while let Some(page) = paginator.next().await {
        let page = page?;
        for obj in page.contents() {
            let key = obj.key().unwrap_or_default().to_owned();
            // Skip any "folder placeholder" objects (zero-byte objects ending in /).
            if key.ends_with('/') {
                continue;
            }
            let name = key.rsplit('/').next().unwrap_or(&key).to_owned();

            let last_modified = obj.last_modified().and_then(|dt| {
                chrono::DateTime::from_timestamp(dt.secs(), dt.subsec_nanos())
                    .map(|d| d.with_timezone(&chrono::Utc))
            });

            entries.push(S3Entry {
                key,
                name,
                size_bytes: u64::try_from(obj.size().unwrap_or(0)).unwrap_or(0),
                last_modified,
                e_tag: obj.e_tag().map(str::to_owned),
                kind: EntryKind::File,
            });
        }
    }

    Ok(entries)
}
