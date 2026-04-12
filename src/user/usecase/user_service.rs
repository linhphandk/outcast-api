use crate::user::repository::user_repository::{RepositoryError, User, UserRepository};

pub struct UserService {
    repository: UserRepository,
}

impl UserService {
    pub fn new(repository: UserRepository) -> Self {
        Self { repository }
    }

    pub async fn create(&self, email: String, password: String) -> Result<User, RepositoryError> {
        self.repository.create(email, password).await
    }
}
