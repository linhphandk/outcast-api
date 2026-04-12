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

#[derive(Clone)]
pub struct UserRepository {
    pool: Pool,
}

impl UserRepository {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, email: String, password: String) -> Result<User, RepositoryError> {
        let conn = self.pool.get().await?;
        let id = Uuid::new_v4();

        let inserted_user = conn
            .interact(move |conn| {
                let new_user = NewUser {
                    id,
                    email: email.clone(),
                    password: password.clone(),
                };

                diesel::insert_into(users::table)
                    .values(&new_user)
                    .execute(conn)?;

                Ok::<_, diesel::result::Error>(User {
                    id,
                    email,
                    password,
                })
            })
            .await??;

        Ok(inserted_user)
    }
}
