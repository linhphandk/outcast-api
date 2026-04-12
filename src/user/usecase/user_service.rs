use crate::user::repository::user_repository::{RepositoryError, User, UserRepositoryTrait};

pub struct UserService<R: UserRepositoryTrait> {
    repository: R,
}

impl<R: UserRepositoryTrait + Clone> Clone for UserService<R> {
    fn clone(&self) -> Self {
        Self {
            repository: self.repository.clone(),
        }
    }
}

impl<R: UserRepositoryTrait> UserService<R> {
    pub fn new(repository: R) -> Self {
        Self { repository }
    }

    pub async fn create(&self, email: String, password: String) -> Result<User, RepositoryError> {
        self.repository.create(email, password).await
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
                eq("password123".to_string()),
            )
            .times(1)
            .returning(|email, password| {
                Ok(User {
                    id: Uuid::nil(),
                    email,
                    password,
                })
            });

        let service = UserService::new(mock);
        let result = service
            .create("test@example.com".to_string(), "password123".to_string())
            .await;

        assert!(result.is_ok());
        let user = result.unwrap();
        assert_eq!(user.email, "test@example.com");
        assert_eq!(user.password, "password123");
    }

    #[tokio::test]
    async fn test_user_create_fail() {
        let mut mock = MockUserRepositoryTrait::new();

        mock.expect_create()
            .with(
                eq("fail@example.com".to_string()),
                eq("password123".to_string()),
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

        let service = UserService::new(mock);
        let result = service
            .create("fail@example.com".to_string(), "password123".to_string())
            .await;

        assert!(result.is_err());
    }
}
