pub mod s3_adapter;

use async_trait::async_trait;
use bytes::Bytes;
#[cfg(test)]
use mockall::{automock, predicate::*};

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("Upload failed: {0}")]
    UploadFailed(String),
    #[error("Download failed: {0}")]
    DownloadFailed(String),
    #[error("Delete failed: {0}")]
    DeleteFailed(String),
    #[error("Presign failed: {0}")]
    PresignFailed(String),
}

#[cfg_attr(test, automock)]
#[async_trait]
pub trait StoragePort: Send + Sync {
    /// Upload data to storage under the given key.
    ///
    /// Returns a storage URI in the form `s3://<bucket>/<key>` identifying the stored object.
    /// Use `generate_presigned_url` to obtain an HTTP-accessible URL for the object.
    async fn upload(&self, key: &str, data: Bytes, content_type: &str)
        -> Result<String, StorageError>;

    /// Download the object stored under the given key, returning its contents.
    async fn download(&self, key: &str) -> Result<Bytes, StorageError>;

    /// Delete the object stored under the given key.
    async fn delete(&self, key: &str) -> Result<(), StorageError>;

    /// Generate a time-limited presigned HTTP URL for downloading the object at `key`.
    ///
    /// `expiry_secs` controls how long the URL remains valid.
    async fn generate_presigned_url(
        &self,
        key: &str,
        expiry_secs: u64,
    ) -> Result<String, StorageError>;
}
