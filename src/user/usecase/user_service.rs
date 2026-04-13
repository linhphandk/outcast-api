use crate::user::crypto::hash_password::hash_password;
use crate::user::repository::user_repository::{RepositoryError, User, UserRepositoryTrait};

pub struct UserService<R: UserRepositoryTrait> {
    repository: R,
    pepper: String,
}

impl<R: UserRepositoryTrait + Clone> Clone for UserService<R> {
    fn clone(&self) -> Self {
        Self {
            repository: self.repository.clone(),
            pepper: self.pepper.clone(),
        }
    }
}

impl<R: UserRepositoryTrait> UserService<R> {
    pub fn new(repository: R, pepper: String) -> Self {
        Self { repository, pepper }
    }

    pub async fn create(&self, email: String, password: String) -> Result<User, RepositoryError> {
        let hashed_password =
            hash_password(&password, &self.pepper).map_err(|_| {
                RepositoryError::DieselError(diesel::result::Error::DatabaseError(
                    diesel::result::DatabaseErrorKind::Unknown,
                    Box::new("Failed to hash password".to_string()),
                ))
            })?;
        self.repository.create(email, hashed_password).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::user::repository::user_repository::MockUserRepositoryTrait;
    use mockall::predicate::*;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_user_create() {
        let mut mock = MockUserRepositoryTrait::new();

        mock.expect_create()
            .with(
                eq("test@example.com".to_string()),
                always(),
            )
            .times(1)
            .returning(|email, password| {
                Ok(User {
                    id: Uuid::nil(),
                    email,
                    password,
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
        assert!(crate::user::crypto::hash_password::verify_password("password123", &user.password, &test_pepper).unwrap());
    }

    #[tokio::test]
    async fn test_user_create_fail() {
        let mut mock = MockUserRepositoryTrait::new();

        mock.expect_create()
            .with(
                eq("fail@example.com".to_string()),
                always(),
            )
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
}
