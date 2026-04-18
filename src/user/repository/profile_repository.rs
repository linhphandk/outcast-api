use crate::schema::{profiles, rates, social_handles};
use async_trait::async_trait;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use deadpool_diesel::InteractError;
use deadpool_diesel::postgres::Pool;
use diesel::prelude::*;
#[cfg(test)]
use mockall::{automock, predicate::*};
use tracing::{debug, error, info, instrument};
use uuid::Uuid;

#[derive(Debug, PartialEq, Clone, Queryable)]
pub struct Profile {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub bio: String,
    pub niche: String,
    pub avatar_url: String,
    pub username: String,
    pub updated_at: Option<DateTime<Utc>>,
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Insertable)]
#[diesel(table_name = profiles)]
pub struct NewProfile {
    pub user_id: Uuid,
    pub name: String,
    pub bio: String,
    pub niche: String,
    pub avatar_url: String,
    pub username: String,
}

#[derive(AsChangeset)]
#[diesel(table_name = profiles)]
pub struct UpdateProfile {
    pub name: String,
    pub bio: String,
    pub niche: String,
    pub avatar_url: String,
    pub username: String,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, PartialEq, Clone, Queryable)]
pub struct SocialHandle {
    pub id: Uuid,
    pub profile_id: Uuid,
    pub platform: String,
    pub handle: String,
    pub url: String,
    pub follower_count: i32,
    pub updated_at: Option<DateTime<Utc>>,
    pub engagement_rate: BigDecimal,
    pub last_synced_at: Option<DateTime<Utc>>,
}

#[derive(Insertable)]
#[diesel(table_name = social_handles)]
pub struct NewSocialHandle {
    pub profile_id: Uuid,
    pub platform: String,
    pub handle: String,
    pub url: String,
    pub follower_count: i32,
}

#[derive(Debug, PartialEq, Clone, Queryable)]
pub struct Rate {
    pub id: Uuid,
    pub profile_id: Uuid,
    pub rate_type: String,
    pub amount: BigDecimal,
}

#[derive(Insertable)]
#[diesel(table_name = rates)]
pub struct NewRate {
    pub profile_id: Uuid,
    pub type_: String,
    pub amount: BigDecimal,
}

#[derive(Debug, PartialEq, Clone)]
pub struct SocialHandleInput {
    pub platform: String,
    pub handle: String,
    pub url: String,
    pub follower_count: i32,
}

#[derive(Debug, PartialEq, Clone)]
pub struct RateInput {
    pub rate_type: String,
    pub amount: BigDecimal,
}

#[derive(Debug, PartialEq)]
pub struct ProfileWithDetails {
    pub profile: Profile,
    pub social_handles: Vec<SocialHandle>,
    pub rates: Vec<Rate>,
}

#[derive(Debug, thiserror::Error)]
pub enum ProfileRepositoryError {
    #[error("Database pool error: {0}")]
    PoolError(#[from] deadpool_diesel::PoolError),
    #[error("Diesel interaction error: {0}")]
    InteractError(#[from] InteractError),
    #[error("Diesel error: {0}")]
    DieselError(#[from] diesel::result::Error),
}

#[derive(Clone)]
pub struct ProfileRepository {
    pool: Pool,
}

#[cfg_attr(test, automock)]
#[async_trait]
pub trait ProfileRepositoryTrait {
    async fn create(
        &self,
        user_id: Uuid,
        name: String,
        bio: String,
        niche: String,
        avatar_url: String,
        username: String,
    ) -> Result<Profile, ProfileRepositoryError>;

    async fn add_social_handle(
        &self,
        profile_id: Uuid,
        platform: String,
        handle: String,
        url: String,
        follower_count: i32,
    ) -> Result<SocialHandle, ProfileRepositoryError>;

    async fn add_rate(
        &self,
        profile_id: Uuid,
        rate_type: String,
        amount: BigDecimal,
    ) -> Result<Rate, ProfileRepositoryError>;

    async fn create_with_details(
        &self,
        user_id: Uuid,
        name: String,
        bio: String,
        niche: String,
        avatar_url: String,
        username: String,
        social_handles: Vec<SocialHandleInput>,
        rates: Vec<RateInput>,
    ) -> Result<ProfileWithDetails, ProfileRepositoryError>;

    async fn find_by_user_id(
        &self,
        user_id: Uuid,
    ) -> Result<Option<Profile>, ProfileRepositoryError>;

    async fn update_by_user_id(
        &self,
        user_id: Uuid,
        name: String,
        bio: String,
        niche: String,
        avatar_url: String,
        username: String,
    ) -> Result<Option<Profile>, ProfileRepositoryError>;

    async fn find_social_handles_by_profile_id(
        &self,
        profile_id: Uuid,
    ) -> Result<Vec<SocialHandle>, ProfileRepositoryError>;

    async fn find_rates_by_profile_id(
        &self,
        profile_id: Uuid,
    ) -> Result<Vec<Rate>, ProfileRepositoryError>;

    async fn update_rate(
        &self,
        rate_id: Uuid,
        profile_id: Uuid,
        amount: BigDecimal,
    ) -> Result<Option<Rate>, ProfileRepositoryError>;

    async fn delete_rate(
        &self,
        rate_id: Uuid,
        profile_id: Uuid,
    ) -> Result<bool, ProfileRepositoryError>;
}

impl ProfileRepository {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ProfileRepositoryTrait for ProfileRepository {
    #[instrument(skip(self, bio, avatar_url), fields(user_id = %user_id, username = %username))]
    async fn create(
        &self,
        user_id: Uuid,
        name: String,
        bio: String,
        niche: String,
        avatar_url: String,
        username: String,
    ) -> Result<Profile, ProfileRepositoryError> {
        info!("Creating profile");
        debug!("Acquiring database connection from pool");
        let conn = self.pool.get().await.map_err(|e| {
            error!(error = %e, "Failed to acquire database connection");
            ProfileRepositoryError::PoolError(e)
        })?;

        let inserted_profile = conn
            .interact(move |conn| {
                let new_profile = NewProfile {
                    user_id,
                    name,
                    bio,
                    niche,
                    avatar_url,
                    username,
                };

                diesel::insert_into(profiles::table)
                    .values(&new_profile)
                    .get_result::<Profile>(conn)
            })
            .await
            .map_err(|e| {
                error!(error = %e, "Interact error during profile creation");
                ProfileRepositoryError::InteractError(e)
            })?
            .map_err(|e| {
                error!(error = %e, "Diesel error during profile creation");
                ProfileRepositoryError::DieselError(e)
            })?;

        info!(profile_id = %inserted_profile.id, "Profile created successfully");
        Ok(inserted_profile)
    }

    #[instrument(skip(self, handle, url), fields(profile_id = %profile_id, platform = %platform))]
    async fn add_social_handle(
        &self,
        profile_id: Uuid,
        platform: String,
        handle: String,
        url: String,
        follower_count: i32,
    ) -> Result<SocialHandle, ProfileRepositoryError> {
        info!("Adding social handle");
        let conn = self.pool.get().await.map_err(|e| {
            error!(error = %e, "Failed to acquire database connection");
            ProfileRepositoryError::PoolError(e)
        })?;

        let inserted = conn
            .interact(move |conn| {
                let new_handle = NewSocialHandle {
                    profile_id,
                    platform,
                    handle,
                    url,
                    follower_count,
                };

                diesel::insert_into(social_handles::table)
                    .values(&new_handle)
                    .get_result::<SocialHandle>(conn)
            })
            .await
            .map_err(|e| {
                error!(error = %e, "Interact error adding social handle");
                ProfileRepositoryError::InteractError(e)
            })?
            .map_err(|e| {
                error!(error = %e, "Diesel error adding social handle");
                ProfileRepositoryError::DieselError(e)
            })?;

        info!(social_handle_id = %inserted.id, "Social handle added successfully");
        Ok(inserted)
    }

    #[instrument(skip(self, amount), fields(profile_id = %profile_id, rate_type = %rate_type))]
    async fn add_rate(
        &self,
        profile_id: Uuid,
        rate_type: String,
        amount: BigDecimal,
    ) -> Result<Rate, ProfileRepositoryError> {
        info!("Adding rate");
        let conn = self.pool.get().await.map_err(|e| {
            error!(error = %e, "Failed to acquire database connection");
            ProfileRepositoryError::PoolError(e)
        })?;

        let inserted = conn
            .interact(move |conn| {
                let new_rate = NewRate {
                    profile_id,
                    type_:rate_type,
                    amount,
                };

                diesel::insert_into(rates::table)
                    .values(&new_rate)
                    .get_result::<Rate>(conn)
            })
            .await
            .map_err(|e| {
                error!(error = %e, "Interact error adding rate");
                ProfileRepositoryError::InteractError(e)
            })?
            .map_err(|e| {
                error!(error = %e, "Diesel error adding rate");
                ProfileRepositoryError::DieselError(e)
            })?;

        info!(rate_id = %inserted.id, "Rate added successfully");
        Ok(inserted)
    }

    #[instrument(skip(self, bio, avatar_url, social_handle_inputs, rate_inputs), fields(user_id = %user_id, username = %username))]
    async fn create_with_details(
        &self,
        user_id: Uuid,
        name: String,
        bio: String,
        niche: String,
        avatar_url: String,
        username: String,
        social_handle_inputs: Vec<SocialHandleInput>,
        rate_inputs: Vec<RateInput>,
    ) -> Result<ProfileWithDetails, ProfileRepositoryError> {
        info!(
            social_handles_count = social_handle_inputs.len(),
            rates_count = rate_inputs.len(),
            "Creating profile with details in transaction"
        );
        let conn = self.pool.get().await.map_err(|e| {
            error!(error = %e, "Failed to acquire database connection");
            ProfileRepositoryError::PoolError(e)
        })?;

        let result = conn
            .interact(move |conn| {
                conn.transaction(|conn| -> Result<ProfileWithDetails, diesel::result::Error> {
                    let new_profile = NewProfile {
                        user_id,
                        name,
                        bio,
                        niche,
                        avatar_url,
                        username,
                    };
                    let profile = diesel::insert_into(profiles::table)
                        .values(&new_profile)
                        .get_result::<Profile>(conn)?;

                    let new_handles: Vec<NewSocialHandle> = social_handle_inputs
                        .into_iter()
                        .map(|input| NewSocialHandle {
                            profile_id: profile.id,
                            platform: input.platform,
                            handle: input.handle,
                            url: input.url,
                            follower_count: input.follower_count,
                        })
                        .collect();
                    let inserted_handles = diesel::insert_into(social_handles::table)
                        .values(&new_handles)
                        .get_results::<SocialHandle>(conn)?;

                    let new_rates: Vec<NewRate> = rate_inputs
                        .into_iter()
                        .map(|input| NewRate {
                            profile_id: profile.id,
                            type_: input.rate_type,
                            amount: input.amount,
                        })
                        .collect();
                    let inserted_rates = diesel::insert_into(rates::table)
                        .values(&new_rates)
                        .get_results::<Rate>(conn)?;

                    Ok(ProfileWithDetails {
                        profile,
                        social_handles: inserted_handles,
                        rates: inserted_rates,
                    })
                })
            })
            .await
            .map_err(|e| {
                error!(error = %e, "Interact error during profile creation with details");
                ProfileRepositoryError::InteractError(e)
            })?
            .map_err(|e| {
                error!(error = %e, "Transaction error during profile creation with details");
                ProfileRepositoryError::DieselError(e)
            })?;

        info!(profile_id = %result.profile.id, "Profile with details created successfully");
        Ok(result)
    }

    #[instrument(skip(self), fields(user_id = %user_id))]
    async fn find_by_user_id(
        &self,
        user_id: Uuid,
    ) -> Result<Option<Profile>, ProfileRepositoryError> {
        debug!("Finding profile by user id");
        let conn = self.pool.get().await.map_err(|e| {
            error!(error = %e, "Failed to acquire database connection");
            ProfileRepositoryError::PoolError(e)
        })?;

        conn.interact(move |conn| {
            profiles::table
                .filter(profiles::user_id.eq(user_id))
                .first::<Profile>(conn)
                .optional()
        })
        .await
        .map_err(|e| {
            error!(error = %e, "Interact error during profile lookup by user id");
            ProfileRepositoryError::InteractError(e)
        })?
        .map_err(|e| {
            error!(error = %e, "Diesel error during profile lookup by user id");
            ProfileRepositoryError::DieselError(e)
        })
    }

    #[instrument(skip(self, name, bio, niche, avatar_url), fields(user_id = %user_id, username = %username))]
    async fn update_by_user_id(
        &self,
        user_id: Uuid,
        name: String,
        bio: String,
        niche: String,
        avatar_url: String,
        username: String,
    ) -> Result<Option<Profile>, ProfileRepositoryError> {
        debug!("Updating profile by user id");
        let conn = self.pool.get().await.map_err(|e| {
            error!(error = %e, "Failed to acquire database connection");
            ProfileRepositoryError::PoolError(e)
        })?;

        let update = UpdateProfile {
            name,
            bio,
            niche,
            avatar_url,
            username,
            updated_at: Some(Utc::now()),
        };

        conn.interact(move |conn| {
            diesel::update(profiles::table.filter(profiles::user_id.eq(user_id)))
                .set(&update)
                .get_result::<Profile>(conn)
                .optional()
        })
        .await
        .map_err(|e| {
            error!(error = %e, "Interact error during profile update by user id");
            ProfileRepositoryError::InteractError(e)
        })?
        .map_err(|e| {
            error!(error = %e, "Diesel error during profile update by user id");
            ProfileRepositoryError::DieselError(e)
        })
    }

    #[instrument(skip(self), fields(profile_id = %profile_id))]
    async fn find_social_handles_by_profile_id(
        &self,
        profile_id: Uuid,
    ) -> Result<Vec<SocialHandle>, ProfileRepositoryError> {
        debug!("Finding social handles by profile id");
        let conn = self.pool.get().await.map_err(|e| {
            error!(error = %e, "Failed to acquire database connection");
            ProfileRepositoryError::PoolError(e)
        })?;

        conn.interact(move |conn| {
            social_handles::table
                .filter(social_handles::profile_id.eq(profile_id))
                .order(social_handles::platform.asc())
                .load::<SocialHandle>(conn)
        })
        .await
        .map_err(|e| {
            error!(error = %e, "Interact error during social handles lookup by profile id");
            ProfileRepositoryError::InteractError(e)
        })?
        .map_err(|e| {
            error!(error = %e, "Diesel error during social handles lookup by profile id");
            ProfileRepositoryError::DieselError(e)
        })
    }

    #[instrument(skip(self), fields(profile_id = %profile_id))]
    async fn find_rates_by_profile_id(
        &self,
        profile_id: Uuid,
    ) -> Result<Vec<Rate>, ProfileRepositoryError> {
        debug!("Finding rates by profile id");
        let conn = self.pool.get().await.map_err(|e| {
            error!(error = %e, "Failed to acquire database connection");
            ProfileRepositoryError::PoolError(e)
        })?;

        conn.interact(move |conn| {
            rates::table
                .filter(rates::profile_id.eq(profile_id))
                .order(rates::type_.asc())
                .load::<Rate>(conn)
        })
        .await
        .map_err(|e| {
            error!(error = %e, "Interact error during rates lookup by profile id");
            ProfileRepositoryError::InteractError(e)
        })?
        .map_err(|e| {
            error!(error = %e, "Diesel error during rates lookup by profile id");
            ProfileRepositoryError::DieselError(e)
        })
    }

    #[instrument(skip(self, amount), fields(rate_id = %rate_id, profile_id = %profile_id))]
    async fn update_rate(
        &self,
        rate_id: Uuid,
        profile_id: Uuid,
        amount: BigDecimal,
    ) -> Result<Option<Rate>, ProfileRepositoryError> {
        debug!("Updating rate");
        let conn = self.pool.get().await.map_err(|e| {
            error!(error = %e, "Failed to acquire database connection");
            ProfileRepositoryError::PoolError(e)
        })?;

        conn.interact(move |conn| {
            diesel::update(
                rates::table
                    .filter(rates::id.eq(rate_id))
                    .filter(rates::profile_id.eq(profile_id)),
            )
            .set(rates::amount.eq(amount))
            .get_result::<Rate>(conn)
            .optional()
        })
        .await
        .map_err(|e| {
            error!(error = %e, "Interact error during rate update");
            ProfileRepositoryError::InteractError(e)
        })?
        .map_err(|e| {
            error!(error = %e, "Diesel error during rate update");
            ProfileRepositoryError::DieselError(e)
        })
    }

    #[instrument(skip(self), fields(rate_id = %rate_id, profile_id = %profile_id))]
    async fn delete_rate(
        &self,
        rate_id: Uuid,
        profile_id: Uuid,
    ) -> Result<bool, ProfileRepositoryError> {
        debug!("Deleting rate");
        let conn = self.pool.get().await.map_err(|e| {
            error!(error = %e, "Failed to acquire database connection");
            ProfileRepositoryError::PoolError(e)
        })?;

        conn.interact(move |conn| {
            diesel::delete(
                rates::table
                    .filter(rates::id.eq(rate_id))
                    .filter(rates::profile_id.eq(profile_id)),
            )
            .execute(conn)
        })
        .await
        .map_err(|e| {
            error!(error = %e, "Interact error during rate deletion");
            ProfileRepositoryError::InteractError(e)
        })?
        .map_err(|e| {
            error!(error = %e, "Diesel error during rate deletion");
            ProfileRepositoryError::DieselError(e)
        })
        .map(|rows_affected| rows_affected > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::user::repository::user_repository::NewUser;
    use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};

    pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

    async fn setup_test_db() -> (
        testcontainers::ContainerAsync<testcontainers_modules::postgres::Postgres>,
        Pool,
    ) {
        use testcontainers::runners::AsyncRunner;
        use testcontainers_modules::postgres::Postgres;

        let container = Postgres::default().start().await.unwrap();
        let port = container.get_host_port_ipv4(5432).await.unwrap();
        let host = container.get_host().await.unwrap();
        let conn_string = format!("postgres://postgres:postgres@{host}:{port}/postgres");

        let manager =
            deadpool_diesel::postgres::Manager::new(conn_string, deadpool_diesel::Runtime::Tokio1);
        let pool = deadpool_diesel::postgres::Pool::builder(manager)
            .build()
            .unwrap();

        let conn = pool.get().await.unwrap();
        conn.interact(|conn| conn.run_pending_migrations(MIGRATIONS).map(|_| ()))
            .await
            .unwrap()
            .unwrap();

        (container, pool)
    }

    async fn create_test_user(pool: &Pool) -> Uuid {
        let conn = pool.get().await.unwrap();
        let id = Uuid::new_v4();
        conn.interact(move |conn| {
            diesel::insert_into(crate::schema::users::table)
                .values(&NewUser {
                    id,
                    email: format!("user-{}@example.com", id),
                    password: "hashed".to_string(),
                })
                .execute(conn)
        })
        .await
        .unwrap()
        .unwrap();
        id
    }

    #[tokio::test]
    async fn test_create_profile() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = ProfileRepository::new(pool);

        let profile = repo
            .create(
                user_id,
                "Alice".to_string(),
                "Tech creator".to_string(),
                "technology".to_string(),
                "https://example.com/avatar.png".to_string(),
                "alice_tech".to_string(),
            )
            .await
            .unwrap();

        assert_eq!(profile.user_id, user_id);
        assert_eq!(profile.name, "Alice");
        assert_eq!(profile.bio, "Tech creator");
        assert_eq!(profile.niche, "technology");
        assert_eq!(profile.avatar_url, "https://example.com/avatar.png");
        assert_eq!(profile.username, "alice_tech");
        assert!(profile.created_at.is_some());
        assert!(profile.updated_at.is_some());
    }

    #[tokio::test]
    async fn test_create_profile_duplicate_username_fails() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = ProfileRepository::new(pool);

        repo.create(
            user_id,
            "Alice".to_string(),
            "Bio".to_string(),
            "niche".to_string(),
            "https://example.com/avatar.png".to_string(),
            "duplicate_user".to_string(),
        )
        .await
        .unwrap();

        let second_user_id = create_test_user(&repo.pool).await;

        let result = repo
            .create(
                second_user_id,
                "Bob".to_string(),
                "Bio".to_string(),
                "niche".to_string(),
                "https://example.com/avatar2.png".to_string(),
                "duplicate_user".to_string(),
            )
            .await;

        assert!(result.is_err(), "Expected duplicate username to fail");
    }

    #[tokio::test]
    async fn test_create_profile_returns_correct_id() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = ProfileRepository::new(pool);

        let profile1 = repo
            .create(
                user_id,
                "User One".to_string(),
                "Bio one".to_string(),
                "niche".to_string(),
                "https://example.com/a.png".to_string(),
                "userone".to_string(),
            )
            .await
            .unwrap();

        let second_user_id = create_test_user(&repo.pool).await;
        let profile2 = repo
            .create(
                second_user_id,
                "User Two".to_string(),
                "Bio two".to_string(),
                "niche".to_string(),
                "https://example.com/b.png".to_string(),
                "usertwo".to_string(),
            )
            .await
            .unwrap();

        assert_ne!(profile1.id, profile2.id);
    }

    async fn create_test_profile(repo: &ProfileRepository, user_id: Uuid) -> Profile {
        repo.create(
            user_id,
            "Alice".to_string(),
            "Tech creator".to_string(),
            "technology".to_string(),
            "https://example.com/avatar.png".to_string(),
            format!("alice_{}", Uuid::new_v4().simple()),
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn test_find_by_user_id_returns_profile() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = ProfileRepository::new(pool);
        let created = create_test_profile(&repo, user_id).await;

        let found = repo.find_by_user_id(user_id).await.unwrap();

        assert!(found.is_some());
        let found = found.unwrap();
        assert_eq!(found.id, created.id);
        assert_eq!(found.user_id, user_id);
    }

    #[tokio::test]
    async fn test_find_by_user_id_returns_none_when_missing() {
        let (_container, pool) = setup_test_db().await;
        let repo = ProfileRepository::new(pool);

        let found = repo.find_by_user_id(Uuid::new_v4()).await.unwrap();

        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_update_by_user_id_success() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = ProfileRepository::new(pool);
        let _created = create_test_profile(&repo, user_id).await;

        let updated = repo
            .update_by_user_id(
                user_id,
                "Updated Name".to_string(),
                "Updated bio".to_string(),
                "updated niche".to_string(),
                "https://example.com/new-avatar.png".to_string(),
                format!("updated_{}", Uuid::new_v4().simple()),
            )
            .await
            .unwrap();

        assert!(updated.is_some());
        let updated = updated.unwrap();
        assert_eq!(updated.user_id, user_id);
        assert_eq!(updated.name, "Updated Name");
        assert_eq!(updated.bio, "Updated bio");
        assert_eq!(updated.niche, "updated niche");
        assert_eq!(updated.avatar_url, "https://example.com/new-avatar.png");
    }

    #[tokio::test]
    async fn test_update_by_user_id_returns_none_when_missing() {
        let (_container, pool) = setup_test_db().await;
        let repo = ProfileRepository::new(pool);

        let updated = repo
            .update_by_user_id(
                Uuid::new_v4(),
                "Name".to_string(),
                "Bio".to_string(),
                "Niche".to_string(),
                "https://example.com/avatar.png".to_string(),
                "username_not_found".to_string(),
            )
            .await
            .unwrap();

        assert!(updated.is_none());
    }

    #[tokio::test]
    async fn test_add_social_handle() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = ProfileRepository::new(pool);
        let profile = create_test_profile(&repo, user_id).await;

        let social_handle = repo
            .add_social_handle(
                profile.id,
                "instagram".to_string(),
                "@alice_tech".to_string(),
                "https://instagram.com/alice_tech".to_string(),
                50_000,
            )
            .await
            .unwrap();

        assert_eq!(social_handle.profile_id, profile.id);
        assert_eq!(social_handle.platform, "instagram");
        assert_eq!(social_handle.handle, "@alice_tech");
        assert_eq!(social_handle.url, "https://instagram.com/alice_tech");
        assert_eq!(social_handle.follower_count, 50_000);
        assert!(social_handle.updated_at.is_some());
    }

    #[tokio::test]
    async fn test_add_social_handle_duplicate_platform_fails() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = ProfileRepository::new(pool);
        let profile = create_test_profile(&repo, user_id).await;

        repo.add_social_handle(
            profile.id,
            "tiktok".to_string(),
            "@alice".to_string(),
            "https://tiktok.com/@alice".to_string(),
            10_000,
        )
        .await
        .unwrap();

        let result = repo
            .add_social_handle(
                profile.id,
                "tiktok".to_string(),
                "@alice_duplicate".to_string(),
                "https://tiktok.com/@alice_duplicate".to_string(),
                20_000,
            )
            .await;

        assert!(result.is_err(), "Expected duplicate platform to fail");
    }

    #[tokio::test]
    async fn test_add_social_handle_multiple_platforms() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = ProfileRepository::new(pool);
        let profile = create_test_profile(&repo, user_id).await;

        let instagram = repo
            .add_social_handle(
                profile.id,
                "instagram".to_string(),
                "@alice_ig".to_string(),
                "https://instagram.com/alice_ig".to_string(),
                1_000,
            )
            .await
            .unwrap();

        let youtube = repo
            .add_social_handle(
                profile.id,
                "youtube".to_string(),
                "@alice_yt".to_string(),
                "https://youtube.com/@alice_yt".to_string(),
                5_000,
            )
            .await
            .unwrap();

        assert_ne!(instagram.id, youtube.id);
        assert_eq!(instagram.platform, "instagram");
        assert_eq!(youtube.platform, "youtube");
    }

    #[tokio::test]
    async fn test_add_rate() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = ProfileRepository::new(pool);
        let profile = create_test_profile(&repo, user_id).await;

        let rate = repo
            .add_rate(
                profile.id,
                "post".to_string(),
                BigDecimal::from(500),
            )
            .await
            .unwrap();

        assert_eq!(rate.profile_id, profile.id);
        assert_eq!(rate.rate_type, "post");
        assert_eq!(rate.amount, BigDecimal::from(500));
    }

    #[tokio::test]
    async fn test_add_rate_multiple_types() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = ProfileRepository::new(pool);
        let profile = create_test_profile(&repo, user_id).await;

        let post_rate = repo
            .add_rate(profile.id, "post".to_string(), BigDecimal::from(500))
            .await
            .unwrap();

        let story_rate = repo
            .add_rate(profile.id, "story".to_string(), BigDecimal::from(200))
            .await
            .unwrap();

        let reel_rate = repo
            .add_rate(profile.id, "reel".to_string(), BigDecimal::from(800))
            .await
            .unwrap();

        assert_ne!(post_rate.id, story_rate.id);
        assert_ne!(story_rate.id, reel_rate.id);
        assert_eq!(post_rate.rate_type, "post");
        assert_eq!(story_rate.rate_type, "story");
        assert_eq!(reel_rate.rate_type, "reel");
    }

    #[tokio::test]
    async fn test_add_rate_duplicate_type_fails() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = ProfileRepository::new(pool);
        let profile = create_test_profile(&repo, user_id).await;

        repo.add_rate(profile.id, "post".to_string(), BigDecimal::from(300))
            .await
            .unwrap();

        let result = repo
            .add_rate(profile.id, "post".to_string(), BigDecimal::from(400))
            .await;

        assert!(result.is_err(), "Expected duplicate rate type to fail");
    }

    #[tokio::test]
    async fn test_add_rate_invalid_type_fails() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = ProfileRepository::new(pool);
        let profile = create_test_profile(&repo, user_id).await;

        let result = repo
            .add_rate(profile.id, "invalid_type".to_string(), BigDecimal::from(100))
            .await;

        assert!(result.is_err(), "Expected invalid rate type to fail");
    }

    #[tokio::test]
    async fn test_create_with_details_success() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = ProfileRepository::new(pool);

        let result = repo
            .create_with_details(
                user_id,
                "Alice".to_string(),
                "Tech creator".to_string(),
                "technology".to_string(),
                "https://example.com/avatar.png".to_string(),
                "alice_tech".to_string(),
                vec![
                    SocialHandleInput {
                        platform: "instagram".to_string(),
                        handle: "@alice_tech".to_string(),
                        url: "https://instagram.com/alice_tech".to_string(),
                        follower_count: 50_000,
                    },
                    SocialHandleInput {
                        platform: "youtube".to_string(),
                        handle: "@alice_yt".to_string(),
                        url: "https://youtube.com/@alice_yt".to_string(),
                        follower_count: 10_000,
                    },
                ],
                vec![
                    RateInput {
                        rate_type: "post".to_string(),
                        amount: BigDecimal::from(500),
                    },
                    RateInput {
                        rate_type: "story".to_string(),
                        amount: BigDecimal::from(200),
                    },
                ],
            )
            .await
            .unwrap();

        assert_eq!(result.profile.user_id, user_id);
        assert_eq!(result.profile.name, "Alice");
        assert_eq!(result.profile.username, "alice_tech");
        assert_eq!(result.social_handles.len(), 2);
        assert_eq!(result.social_handles[0].platform, "instagram");
        assert_eq!(result.social_handles[1].platform, "youtube");
        assert_eq!(result.rates.len(), 2);
        assert_eq!(result.rates[0].rate_type, "post");
        assert_eq!(result.rates[1].rate_type, "story");
        assert_eq!(result.social_handles[0].profile_id, result.profile.id);
        assert_eq!(result.rates[0].profile_id, result.profile.id);
    }

    #[tokio::test]
    async fn test_create_with_details_no_handles_no_rates() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = ProfileRepository::new(pool);

        let result = repo
            .create_with_details(
                user_id,
                "Bob".to_string(),
                "Gaming".to_string(),
                "gaming".to_string(),
                "https://example.com/bob.png".to_string(),
                "bob_games".to_string(),
                vec![],
                vec![],
            )
            .await
            .unwrap();

        assert_eq!(result.profile.user_id, user_id);
        assert_eq!(result.profile.username, "bob_games");
        assert!(result.social_handles.is_empty());
        assert!(result.rates.is_empty());
    }

    #[tokio::test]
    async fn test_create_with_details_rolls_back_on_duplicate_handle() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = ProfileRepository::new(pool.clone());

        let result = repo
            .create_with_details(
                user_id,
                "Charlie".to_string(),
                "Food blogger".to_string(),
                "food".to_string(),
                "https://example.com/charlie.png".to_string(),
                "charlie_food".to_string(),
                vec![
                    SocialHandleInput {
                        platform: "instagram".to_string(),
                        handle: "@charlie".to_string(),
                        url: "https://instagram.com/charlie".to_string(),
                        follower_count: 1_000,
                    },
                    SocialHandleInput {
                        platform: "instagram".to_string(),
                        handle: "@charlie_dup".to_string(),
                        url: "https://instagram.com/charlie_dup".to_string(),
                        follower_count: 2_000,
                    },
                ],
                vec![],
            )
            .await;

        assert!(result.is_err(), "Expected duplicate platform to fail");

        let conn = pool.get().await.unwrap();
        let count: i64 = conn
            .interact(|conn| {
                crate::schema::profiles::table
                    .count()
                    .get_result::<i64>(conn)
            })
            .await
            .unwrap()
            .unwrap();

        assert_eq!(count, 0, "Profile should have been rolled back");
    }

    // ── find_social_handles_by_profile_id ────────────────────────────────────

    #[tokio::test]
    async fn test_find_social_handles_empty_when_none_exist() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = ProfileRepository::new(pool);
        let profile = create_test_profile(&repo, user_id).await;

        let handles = repo
            .find_social_handles_by_profile_id(profile.id)
            .await
            .unwrap();

        assert!(handles.is_empty());
    }

    #[tokio::test]
    async fn test_find_social_handles_returns_only_own_profile_rows() {
        let (_container, pool) = setup_test_db().await;
        let user_a = create_test_user(&pool).await;
        let user_b = create_test_user(&pool).await;
        let repo = ProfileRepository::new(pool);
        let profile_a = create_test_profile(&repo, user_a).await;
        let profile_b = create_test_profile(&repo, user_b).await;

        repo.add_social_handle(
            profile_a.id,
            "instagram".to_string(),
            "@alice".to_string(),
            "https://instagram.com/alice".to_string(),
            1_000,
        )
        .await
        .unwrap();

        repo.add_social_handle(
            profile_b.id,
            "tiktok".to_string(),
            "@bob".to_string(),
            "https://tiktok.com/@bob".to_string(),
            2_000,
        )
        .await
        .unwrap();

        let handles_a = repo
            .find_social_handles_by_profile_id(profile_a.id)
            .await
            .unwrap();

        assert_eq!(handles_a.len(), 1);
        assert_eq!(handles_a[0].profile_id, profile_a.id);
        assert_eq!(handles_a[0].platform, "instagram");
    }

    #[tokio::test]
    async fn test_find_social_handles_ordered_by_platform() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = ProfileRepository::new(pool);
        let profile = create_test_profile(&repo, user_id).await;

        // Insert in reverse alphabetical order.
        for (platform, handle, url) in [
            ("youtube", "@alice_yt", "https://youtube.com/@alice_yt"),
            ("tiktok", "@alice_tk", "https://tiktok.com/@alice_tk"),
            ("instagram", "@alice_ig", "https://instagram.com/alice_ig"),
        ] {
            repo.add_social_handle(
                profile.id,
                platform.to_string(),
                handle.to_string(),
                url.to_string(),
                1_000,
            )
            .await
            .unwrap();
        }

        let handles = repo
            .find_social_handles_by_profile_id(profile.id)
            .await
            .unwrap();

        assert_eq!(handles.len(), 3);
        assert_eq!(handles[0].platform, "instagram");
        assert_eq!(handles[1].platform, "tiktok");
        assert_eq!(handles[2].platform, "youtube");
    }

    // ── find_rates_by_profile_id ──────────────────────────────────────────────

    #[tokio::test]
    async fn test_find_rates_empty_when_none_exist() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = ProfileRepository::new(pool);
        let profile = create_test_profile(&repo, user_id).await;

        let rates = repo.find_rates_by_profile_id(profile.id).await.unwrap();

        assert!(rates.is_empty());
    }

    #[tokio::test]
    async fn test_find_rates_returns_only_own_profile_rows() {
        let (_container, pool) = setup_test_db().await;
        let user_a = create_test_user(&pool).await;
        let user_b = create_test_user(&pool).await;
        let repo = ProfileRepository::new(pool);
        let profile_a = create_test_profile(&repo, user_a).await;
        let profile_b = create_test_profile(&repo, user_b).await;

        repo.add_rate(profile_a.id, "post".to_string(), BigDecimal::from(500))
            .await
            .unwrap();

        repo.add_rate(profile_b.id, "story".to_string(), BigDecimal::from(200))
            .await
            .unwrap();

        let rates_a = repo.find_rates_by_profile_id(profile_a.id).await.unwrap();

        assert_eq!(rates_a.len(), 1);
        assert_eq!(rates_a[0].profile_id, profile_a.id);
        assert_eq!(rates_a[0].rate_type, "post");
    }

    #[tokio::test]
    async fn test_find_rates_ordered_by_type() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = ProfileRepository::new(pool);
        let profile = create_test_profile(&repo, user_id).await;

        // Insert in reverse alphabetical order.
        for (rate_type, amount) in [("story", 200), ("reel", 800), ("post", 500)] {
            repo.add_rate(profile.id, rate_type.to_string(), BigDecimal::from(amount))
                .await
                .unwrap();
        }

        let rates = repo.find_rates_by_profile_id(profile.id).await.unwrap();

        assert_eq!(rates.len(), 3);
        assert_eq!(rates[0].rate_type, "post");
        assert_eq!(rates[1].rate_type, "reel");
        assert_eq!(rates[2].rate_type, "story");
    }

    // ── update_rate ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_update_rate_returns_updated_rate() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = ProfileRepository::new(pool);
        let profile = create_test_profile(&repo, user_id).await;
        let rate = repo
            .add_rate(profile.id, "post".to_string(), BigDecimal::from(500))
            .await
            .unwrap();

        let updated = repo
            .update_rate(rate.id, profile.id, BigDecimal::from(750))
            .await
            .unwrap();

        assert!(updated.is_some());
        let updated = updated.unwrap();
        assert_eq!(updated.id, rate.id);
        assert_eq!(updated.profile_id, profile.id);
        assert_eq!(updated.rate_type, "post");
        assert_eq!(updated.amount, BigDecimal::from(750));
    }

    #[tokio::test]
    async fn test_update_rate_wrong_profile_id_returns_none() {
        let (_container, pool) = setup_test_db().await;
        let user_a = create_test_user(&pool).await;
        let user_b = create_test_user(&pool).await;
        let repo = ProfileRepository::new(pool);
        let profile_a = create_test_profile(&repo, user_a).await;
        let profile_b = create_test_profile(&repo, user_b).await;
        let rate = repo
            .add_rate(profile_a.id, "post".to_string(), BigDecimal::from(500))
            .await
            .unwrap();

        // Supply profile_b's id — must not touch profile_a's rate.
        let result = repo
            .update_rate(rate.id, profile_b.id, BigDecimal::from(999))
            .await
            .unwrap();

        assert!(result.is_none());

        // Confirm the original amount is unchanged.
        let rates = repo.find_rates_by_profile_id(profile_a.id).await.unwrap();
        assert_eq!(rates[0].amount, BigDecimal::from(500));
    }

    #[tokio::test]
    async fn test_update_rate_nonexistent_returns_none() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = ProfileRepository::new(pool);
        let profile = create_test_profile(&repo, user_id).await;

        let result = repo
            .update_rate(Uuid::new_v4(), profile.id, BigDecimal::from(100))
            .await
            .unwrap();

        assert!(result.is_none());
    }

    // ── delete_rate ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_delete_rate_returns_true() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = ProfileRepository::new(pool);
        let profile = create_test_profile(&repo, user_id).await;
        let rate = repo
            .add_rate(profile.id, "post".to_string(), BigDecimal::from(500))
            .await
            .unwrap();

        let deleted = repo.delete_rate(rate.id, profile.id).await.unwrap();

        assert!(deleted);
        let rates = repo.find_rates_by_profile_id(profile.id).await.unwrap();
        assert!(rates.is_empty());
    }

    #[tokio::test]
    async fn test_delete_rate_wrong_profile_id_returns_false() {
        let (_container, pool) = setup_test_db().await;
        let user_a = create_test_user(&pool).await;
        let user_b = create_test_user(&pool).await;
        let repo = ProfileRepository::new(pool);
        let profile_a = create_test_profile(&repo, user_a).await;
        let profile_b = create_test_profile(&repo, user_b).await;
        let rate = repo
            .add_rate(profile_a.id, "post".to_string(), BigDecimal::from(500))
            .await
            .unwrap();

        // Supply profile_b's id — must not delete profile_a's rate.
        let deleted = repo.delete_rate(rate.id, profile_b.id).await.unwrap();

        assert!(!deleted);
        let rates = repo.find_rates_by_profile_id(profile_a.id).await.unwrap();
        assert_eq!(rates.len(), 1);
    }

    #[tokio::test]
    async fn test_delete_rate_nonexistent_returns_false() {
        let (_container, pool) = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        let repo = ProfileRepository::new(pool);
        let profile = create_test_profile(&repo, user_id).await;

        let deleted = repo.delete_rate(Uuid::new_v4(), profile.id).await.unwrap();

        assert!(!deleted);
    }
}
