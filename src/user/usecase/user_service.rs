use std::sync::Arc;

use bytes::Bytes;
use crate::user::crypto::hash_password::{hash_password, verify_password};
use crate::user::repository::user_repository::{RepositoryError, User, UserRepositoryTrait};
use crate::user::storage::{StorageError, StoragePort};

use tracing::{debug, error, info, instrument, warn};
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("Repository error: {0}")]
    RepositoryError(#[from] RepositoryError),
    #[error("User not found")]
    UserNotFound,
    #[error("Invalid credentials")]
    InvalidCredentials,
    #[error("Password hash error: {0}")]
    HashError(#[from] bcrypt::BcryptError),
    #[error("Storage error: {0}")]
    StorageError(#[from] StorageError),
    #[error("Storage is not configured")]
    StorageNotConfigured,
}

pub struct UserService<R: UserRepositoryTrait> {
    repository: R,
    pepper: String,
    storage: Option<Arc<dyn StoragePort>>,
}

impl<R: UserRepositoryTrait + Clone> Clone for UserService<R> {
    fn clone(&self) -> Self {
        Self {
            repository: self.repository.clone(),
            pepper: self.pepper.clone(),
            storage: self.storage.clone(),
        }
    }
}

impl<R: UserRepositoryTrait> UserService<R> {
    pub fn new(repository: R, pepper: String) -> Self {
        Self {
            repository,
            pepper,
            storage: None,
        }
    }

    pub fn new_with_storage(repository: R, pepper: String, storage: Arc<dyn StoragePort>) -> Self {
        Self {
            repository,
            pepper,
            storage: Some(storage),
        }
    }

    #[instrument(skip_all)]
    pub async fn create(&self, email: String, password: String) -> Result<User, RepositoryError> {
        debug!("Hashing password for new user");
        let hashed_password = hash_password(&password, &self.pepper).map_err(|e| {
            error!(error = %e, "Failed to hash password during user creation");
            RepositoryError::DieselError(diesel::result::Error::DatabaseError(
                diesel::result::DatabaseErrorKind::Unknown,
                Box::new("Failed to hash password".to_string()),
            ))
        })?;
        info!("Password hashed, creating user in repository");
        self.repository.create(email, hashed_password).await
    }

    #[instrument(skip_all)]
    pub async fn authenticate(
        &self,
        email: String,
        password: String,
    ) -> Result<User, ServiceError> {
        debug!("Authenticating user");
        let user = self
            .repository
            .find_by_email(email)
            .await
            .map_err(ServiceError::RepositoryError)?
            .ok_or_else(|| {
                warn!("Authentication failed: user not found");
                ServiceError::UserNotFound
            })?;

        let is_valid = verify_password(&password, &user.password, &self.pepper)
            .map_err(ServiceError::HashError)?;

        if !is_valid {
            warn!(user_id = %user.id, "Authentication failed: invalid password");
            return Err(ServiceError::InvalidCredentials);
        }

        info!(user_id = %user.id, "User authenticated successfully");
        Ok(user)
    }

    #[instrument(skip(self), fields(user_id = %user_id))]
    pub async fn get_me(&self, user_id: Uuid) -> Result<User, ServiceError> {
        debug!("Fetching user by ID");
        self.repository
            .find_by_id(user_id)
            .await
            .map_err(ServiceError::RepositoryError)?
            .ok_or_else(|| {
                warn!(user_id = %user_id, "User not found");
                ServiceError::UserNotFound
            })
    }

    #[instrument(skip(self, data), fields(user_id = %user_id))]
    pub async fn upload_avatar(
        &self,
        user_id: Uuid,
        data: Bytes,
        content_type: &str,
    ) -> Result<String, ServiceError> {
        let storage = self
            .storage
            .as_ref()
            .ok_or(ServiceError::StorageNotConfigured)?;
        let key = format!("avatars/{user_id}");
        let avatar_url = storage.upload(&key, data, content_type).await?;
        self.repository
            .update_avatar_url(user_id, &avatar_url)
            .await
            .map_err(ServiceError::RepositoryError)?;
        Ok(avatar_url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use crate::user::repository::user_repository::MockUserRepositoryTrait;
    use crate::user::storage::MockStoragePort;
    use bytes::Bytes;
    use mockall::predicate::*;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_user_create() {
        let mut mock = MockUserRepositoryTrait::new();

        mock.expect_create()
            .with(eq("test@example.com".to_string()), always())
            .times(1)
            .returning(|email, password| {
                Ok(User {
                    id: Uuid::nil(),
                    email,
                    password,
                    avatar_url: None,
                })
            });

        let test_pepper = "test_pepper".to_string();
        let service = UserService::new(mock, test_pepper.clone());
        let result = service
            .create("test@example.com".to_string(), "password123".to_string())
            .await;

        assert!(result.is_ok());
        let user = result.unwrap();
        assert_eq!(user.email, "test@example.com");
        // Verify it's actually hashed with pepper
        assert!(
            crate::user::crypto::hash_password::verify_password(
                "password123",
                &user.password,
                &test_pepper
            )
            .unwrap()
        );
    }

    #[tokio::test]
    async fn test_user_create_fail() {
        let mut mock = MockUserRepositoryTrait::new();

        mock.expect_create()
            .with(eq("fail@example.com".to_string()), always())
            .times(1)
            .returning(|_, _| {
                Err(RepositoryError::DieselError(
                    diesel::result::Error::DatabaseError(
                        diesel::result::DatabaseErrorKind::UniqueViolation,
                        Box::new("duplicate key".to_string()),
                    ),
                ))
            });

        let service = UserService::new(mock, "test_pepper".to_string());
        let result = service
            .create("fail@example.com".to_string(), "password123".to_string())
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_authenticate_success() {
        let test_pepper = "test_pepper".to_string();
        let hashed = crate::user::crypto::hash_password::hash_password("password123", &test_pepper)
            .unwrap();

        let mut mock = MockUserRepositoryTrait::new();
        mock.expect_find_by_email()
            .with(eq("user@example.com".to_string()))
            .times(1)
            .returning(move |email| {
                Ok(Some(User {
                    id: Uuid::nil(),
                    email,
                    password: hashed.clone(),
                    avatar_url: None,
                }))
            });

        let service = UserService::new(mock, test_pepper);
        let result = service
            .authenticate("user@example.com".to_string(), "password123".to_string())
            .await;

        assert!(result.is_ok());
        let user = result.unwrap();
        assert_eq!(user.email, "user@example.com");
    }

    #[tokio::test]
    async fn test_authenticate_user_not_found() {
        let mut mock = MockUserRepositoryTrait::new();
        mock.expect_find_by_email()
            .with(eq("missing@example.com".to_string()))
            .times(1)
            .returning(|_| Ok(None));

        let service = UserService::new(mock, "test_pepper".to_string());
        let result = service
            .authenticate("missing@example.com".to_string(), "password123".to_string())
            .await;

        assert!(matches!(result, Err(ServiceError::UserNotFound)));
    }

    #[tokio::test]
    async fn test_authenticate_wrong_password() {
        let test_pepper = "test_pepper".to_string();
        let hashed = crate::user::crypto::hash_password::hash_password("correct_password", &test_pepper)
            .unwrap();

        let mut mock = MockUserRepositoryTrait::new();
        mock.expect_find_by_email()
            .with(eq("user@example.com".to_string()))
            .times(1)
            .returning(move |email| {
                Ok(Some(User {
                    id: Uuid::nil(),
                    email,
                    password: hashed.clone(),
                    avatar_url: None,
                }))
            });

        let service = UserService::new(mock, test_pepper);
        let result = service
            .authenticate("user@example.com".to_string(), "wrong_password".to_string())
            .await;

        assert!(matches!(result, Err(ServiceError::InvalidCredentials)));
    }

    #[tokio::test]
    async fn test_authenticate_repository_error() {
        let mut mock = MockUserRepositoryTrait::new();
        mock.expect_find_by_email()
            .with(eq("user@example.com".to_string()))
            .times(1)
            .returning(|_| {
                Err(RepositoryError::DieselError(
                    diesel::result::Error::DatabaseError(
                        diesel::result::DatabaseErrorKind::Unknown,
                        Box::new("connection error".to_string()),
                    ),
                ))
            });

        let service = UserService::new(mock, "test_pepper".to_string());
        let result = service
            .authenticate("user@example.com".to_string(), "password123".to_string())
            .await;

        assert!(matches!(result, Err(ServiceError::RepositoryError(_))));
    }

    #[tokio::test]
    async fn test_get_me_success() {
        let user_id = Uuid::new_v4();
        let mut mock = MockUserRepositoryTrait::new();
        mock.expect_find_by_id()
            .with(eq(user_id))
            .times(1)
            .returning(move |id| {
                Ok(Some(User {
                    id,
                    email: "me@example.com".to_string(),
                    password: "hashed".to_string(),
                    avatar_url: None,
                }))
            });

        let service = UserService::new(mock, "test_pepper".to_string());
        let result = service.get_me(user_id).await;

        assert!(result.is_ok());
        let user = result.unwrap();
        assert_eq!(user.id, user_id);
        assert_eq!(user.email, "me@example.com");
    }

    #[tokio::test]
    async fn test_get_me_not_found() {
        let user_id = Uuid::new_v4();
        let mut mock = MockUserRepositoryTrait::new();
        mock.expect_find_by_id()
            .with(eq(user_id))
            .times(1)
            .returning(|_| Ok(None));

        let service = UserService::new(mock, "test_pepper".to_string());
        let result = service.get_me(user_id).await;

        assert!(matches!(result, Err(ServiceError::UserNotFound)));
    }

    #[tokio::test]
    async fn test_get_me_repository_error() {
        let user_id = Uuid::new_v4();
        let mut mock = MockUserRepositoryTrait::new();
        mock.expect_find_by_id()
            .with(eq(user_id))
            .times(1)
            .returning(|_| {
                Err(RepositoryError::DieselError(
                    diesel::result::Error::DatabaseError(
                        diesel::result::DatabaseErrorKind::Unknown,
                        Box::new("connection error".to_string()),
                    ),
                ))
            });

        let service = UserService::new(mock, "test_pepper".to_string());
        let result = service.get_me(user_id).await;

        assert!(matches!(result, Err(ServiceError::RepositoryError(_))));
    }

    #[tokio::test]
    async fn test_upload_avatar_success() {
        let user_id = Uuid::new_v4();
        let mut mock_repo = MockUserRepositoryTrait::new();
        let mut mock_storage = MockStoragePort::new();

        mock_storage
            .expect_upload()
            .withf(|key, _data, content_type| {
                key.starts_with("avatars/") && content_type == "image/png"
            })
            .times(1)
            .returning(|_, _, _| Ok("s3://test-bucket/avatars/user.png".to_string()));

        mock_repo
            .expect_update_avatar_url()
            .with(eq(user_id), eq("s3://test-bucket/avatars/user.png"))
            .times(1)
            .returning(move |user_id, url| {
                Ok(User {
                    id: user_id,
                    email: "user@example.com".to_string(),
                    password: "hashed".to_string(),
                    avatar_url: Some(url.to_string()),
                })
            });

        let service =
            UserService::new_with_storage(mock_repo, "test_pepper".to_string(), Arc::new(mock_storage));
        let result = service
            .upload_avatar(
                user_id,
                Bytes::from_static(b"png-data"),
                "image/png",
            )
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "s3://test-bucket/avatars/user.png");
    }

    #[tokio::test]
    async fn test_upload_avatar_storage_error() {
        let user_id = Uuid::new_v4();
        let mock_repo = MockUserRepositoryTrait::new();
        let mut mock_storage = MockStoragePort::new();

        mock_storage
            .expect_upload()
            .times(1)
            .returning(|_, _, _| Err(StorageError::UploadFailed("s3 unavailable".to_string())));

        let service =
            UserService::new_with_storage(mock_repo, "test_pepper".to_string(), Arc::new(mock_storage));
        let result = service
            .upload_avatar(
                user_id,
                Bytes::from_static(b"png-data"),
                "image/png",
            )
            .await;

        assert!(matches!(result, Err(ServiceError::StorageError(_))));
    }
}
