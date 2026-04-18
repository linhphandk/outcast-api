use async_trait::async_trait;
use aws_sdk_s3::Client;
use aws_sdk_s3::presigning::PresigningConfig;
use bytes::Bytes;
use std::time::Duration;
use tracing::{debug, error, instrument};

use super::{StorageError, StoragePort};

#[derive(Clone)]
pub struct S3Adapter {
    client: Client,
    bucket: String,
}

impl S3Adapter {
    pub fn new(client: Client, bucket: String) -> Self {
        Self { client, bucket }
    }
}

#[async_trait]
impl StoragePort for S3Adapter {
    #[instrument(skip(self, data), fields(bucket = %self.bucket, key = %key, content_type = %content_type))]
    async fn upload(
        &self,
        key: &str,
        data: Bytes,
        content_type: &str,
    ) -> Result<String, StorageError> {
        debug!("Uploading object to S3");

        let body = aws_sdk_s3::primitives::ByteStream::from(data);

        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(body)
            .content_type(content_type)
            .send()
            .await
            .map_err(|e| {
                error!(error = %e, "S3 upload failed");
                StorageError::UploadFailed(e.to_string())
            })?;

        let url = format!("s3://{}/{}", self.bucket, key);
        debug!(url = %url, "Upload complete");
        Ok(url)
    }

    #[instrument(skip(self), fields(bucket = %self.bucket, key = %key))]
    async fn download(&self, key: &str) -> Result<Bytes, StorageError> {
        debug!("Downloading object from S3");

        let resp = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| {
                error!(error = %e, "S3 download failed");
                StorageError::DownloadFailed(e.to_string())
            })?;

        let data = resp.body.collect().await.map_err(|e| {
            error!(error = %e, "Failed to read S3 response body");
            StorageError::DownloadFailed(e.to_string())
        })?;

        debug!("Download complete");
        Ok(data.into_bytes())
    }

    #[instrument(skip(self), fields(bucket = %self.bucket, key = %key))]
    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        debug!("Deleting object from S3");

        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| {
                error!(error = %e, "S3 delete failed");
                StorageError::DeleteFailed(e.to_string())
            })?;

        debug!("Delete complete");
        Ok(())
    }

    #[instrument(skip(self), fields(bucket = %self.bucket, key = %key, expiry_secs = expiry_secs))]
    async fn generate_presigned_url(
        &self,
        key: &str,
        expiry_secs: u64,
    ) -> Result<String, StorageError> {
        debug!("Generating presigned URL");

        let presigning_config =
            PresigningConfig::expires_in(Duration::from_secs(expiry_secs)).map_err(|e| {
                error!(error = %e, "Failed to build presigning config");
                StorageError::PresignFailed(e.to_string())
            })?;

        let presigned = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .presigned(presigning_config)
            .await
            .map_err(|e| {
                error!(error = %e, "S3 presign failed");
                StorageError::PresignFailed(e.to_string())
            })?;

        let url = presigned.uri().to_string();
        debug!(url = %url, "Presigned URL generated");
        Ok(url)
    }
}
