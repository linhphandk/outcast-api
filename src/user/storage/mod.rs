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
    async fn upload(&self, key: &str, data: Bytes, content_type: &str)
        -> Result<String, StorageError>;
    async fn download(&self, key: &str) -> Result<Bytes, StorageError>;
    async fn delete(&self, key: &str) -> Result<(), StorageError>;
    async fn generate_presigned_url(
        &self,
        key: &str,
        expiry_secs: u64,
    ) -> Result<String, StorageError>;
}
