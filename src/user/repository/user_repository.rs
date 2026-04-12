use crate::schema::users;
use deadpool_diesel::InteractError;
use deadpool_diesel::postgres::Pool;
use diesel::prelude::*;
use uuid::Uuid;

pub struct User {
    pub id: Uuid,
    pub email: String,
    pub password: String,
}

#[derive(Insertable)]
#[diesel(table_name = users)]
pub struct NewUser {
    pub id: Uuid,
    pub email: String,
    pub password: String,
}

#[derive(Debug, thiserror::Error)]
pub enum RepositoryError {
    #[error("Database pool error: {0}")]
    PoolError(#[from] deadpool_diesel::PoolError),
    #[error("Diesel interaction error: {0}")]
    InteractError(#[from] InteractError),
    #[error("Diesel error: {0}")]
    DieselError(#[from] diesel::result::Error),
}

pub struct UserRepository {
    pool: Pool,
}

impl UserRepository {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, email: &str, password: &str) -> Result<User, RepositoryError> {
        let conn = self.pool.get().await?;
        let id = Uuid::new_v4();
        let email_owned = email.to_string();
        let password_owned = password.to_string();

        let inserted_user = conn
            .interact(move |conn| {
                let new_user = NewUser {
                    id,
                    email: email_owned.clone(),
                    password: password_owned.clone(),
                };

                diesel::insert_into(users::table)
                    .values(&new_user)
                    .execute(conn)?;

                Ok::<_, diesel::result::Error>(User {
                    id,
                    email: email_owned,
                    password: password_owned,
                })
            })
            .await??;

        Ok(inserted_user)
    }
}
