use tracing::{error, info, instrument};
use uuid::Uuid;

use crate::user::repository::profile_repository::{
    ProfileRepositoryError, ProfileRepositoryTrait, ProfileWithDetails, RateInput,
    SocialHandleInput,
};

#[derive(Debug, thiserror::Error)]
pub enum ProfileServiceError {
    #[error("Repository error: {0}")]
    RepositoryError(#[from] ProfileRepositoryError),
    #[error("Profile not found")]
    ProfileNotFound,
}

pub struct ProfileService<R: ProfileRepositoryTrait> {
    repository: R,
}

impl<R: ProfileRepositoryTrait + Clone> Clone for ProfileService<R> {
    fn clone(&self) -> Self {
        Self {
            repository: self.repository.clone(),
        }
    }
}

impl<R: ProfileRepositoryTrait> ProfileService<R> {
    pub fn new(repository: R) -> Self {
        Self { repository }
    }

    #[instrument(skip(self, bio, avatar_url, social_handles, rates), fields(user_id = %user_id, username = %username))]
    pub async fn add_profile(
        &self,
        user_id: Uuid,
        name: String,
        bio: String,
        niche: String,
        avatar_url: String,
        username: String,
        social_handles: Vec<SocialHandleInput>,
        rates: Vec<RateInput>,
    ) -> Result<ProfileWithDetails, ProfileServiceError> {
        info!(
            social_handles_count = social_handles.len(),
            rates_count = rates.len(),
            "Creating profile with details"
        );
        let result = self
            .repository
            .create_with_details(
                user_id,
                name,
                bio,
                niche,
                avatar_url,
                username,
                social_handles,
                rates,
            )
            .await
            .map_err(|e| {
                error!(error = %e, "Failed to create profile");
                ProfileServiceError::RepositoryError(e)
            })?;
        info!(profile_id = %result.profile.id, "Profile created successfully");
        Ok(result)
    }

    #[instrument(skip(self), fields(user_id = %user_id))]
    pub async fn get_profile_by_user_id(&self, user_id: Uuid) -> Result<crate::user::repository::profile_repository::Profile, ProfileServiceError> {
        self.repository
            .find_by_user_id(user_id)
            .await
            .map_err(|e| {
                error!(error = %e, "Failed to get profile");
                ProfileServiceError::RepositoryError(e)
            })?
            .ok_or_else(|| {
                ProfileServiceError::ProfileNotFound
            })
    }

    #[instrument(skip(self, name, bio, niche, avatar_url), fields(user_id = %user_id, username = %username))]
    pub async fn update_profile_by_user_id(
        &self,
        user_id: Uuid,
        name: String,
        bio: String,
        niche: String,
        avatar_url: String,
        username: String,
    ) -> Result<crate::user::repository::profile_repository::Profile, ProfileServiceError> {
        self.repository
            .update_by_user_id(user_id, name, bio, niche, avatar_url, username)
            .await
            .map_err(|e| {
                error!(error = %e, "Failed to update profile");
                ProfileServiceError::RepositoryError(e)
            })?
            .ok_or_else(|| {
                ProfileServiceError::ProfileNotFound
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::user::repository::profile_repository::{
        MockProfileRepositoryTrait, Profile, Rate, SocialHandle,
    };
    use bigdecimal::BigDecimal;
    use chrono::Utc;
    use mockall::predicate::eq;
    use uuid::Uuid;

    fn make_profile(user_id: Uuid) -> Profile {
        Profile {
            id: Uuid::new_v4(),
            user_id,
            name: "Alice".to_string(),
            bio: "Tech creator".to_string(),
            niche: "technology".to_string(),
            avatar_url: "https://example.com/avatar.png".to_string(),
            username: "alice_tech".to_string(),
            updated_at: Some(Utc::now()),
            created_at: Some(Utc::now()),
        }
    }

    fn make_social_handle(profile_id: Uuid) -> SocialHandle {
        SocialHandle {
            id: Uuid::new_v4(),
            profile_id,
            platform: "instagram".to_string(),
            handle: "@alice_tech".to_string(),
            url: "https://instagram.com/alice_tech".to_string(),
            follower_count: 50_000,
            updated_at: Some(Utc::now()),
        }
    }

    fn make_rate(profile_id: Uuid) -> Rate {
        Rate {
            id: Uuid::new_v4(),
            profile_id,
            rate_type: "post".to_string(),
            amount: BigDecimal::from(500),
        }
    }

    #[tokio::test]
    async fn test_add_profile_success() {
        let user_id = Uuid::new_v4();
        let profile = make_profile(user_id);
        let social_handle = make_social_handle(profile.id);
        let rate = make_rate(profile.id);

        let expected = ProfileWithDetails {
            profile: Profile { ..profile.clone() },
            social_handles: vec![SocialHandle { ..social_handle.clone() }],
            rates: vec![Rate { ..rate.clone() }],
        };

        let mut mock = MockProfileRepositoryTrait::new();
        mock.expect_create_with_details()
            .times(1)
            .returning(move |_, _, _, _, _, _, _, _| {
                Ok(ProfileWithDetails {
                    profile: Profile { ..profile.clone() },
                    social_handles: vec![SocialHandle { ..social_handle.clone() }],
                    rates: vec![Rate { ..rate.clone() }],
                })
            });

        let service = ProfileService::new(mock);
        let result = service
            .add_profile(
                user_id,
                "Alice".to_string(),
                "Tech creator".to_string(),
                "technology".to_string(),
                "https://example.com/avatar.png".to_string(),
                "alice_tech".to_string(),
                vec![SocialHandleInput {
                    platform: "instagram".to_string(),
                    handle: "@alice_tech".to_string(),
                    url: "https://instagram.com/alice_tech".to_string(),
                    follower_count: 50_000,
                }],
                vec![RateInput {
                    rate_type: "post".to_string(),
                    amount: BigDecimal::from(500),
                }],
            )
            .await;

        assert!(result.is_ok());
        let details = result.unwrap();
        assert_eq!(details.profile, expected.profile);
        assert_eq!(details.social_handles, expected.social_handles);
        assert_eq!(details.rates, expected.rates);
    }

    #[tokio::test]
    async fn test_add_profile_repository_error() {
        let user_id = Uuid::new_v4();

        let mut mock = MockProfileRepositoryTrait::new();
        mock.expect_create_with_details()
            .times(1)
            .returning(|_, _, _, _, _, _, _, _| {
                Err(ProfileRepositoryError::DieselError(
                    diesel::result::Error::DatabaseError(
                        diesel::result::DatabaseErrorKind::UniqueViolation,
                        Box::new("duplicate key".to_string()),
                    ),
                ))
            });

        let service = ProfileService::new(mock);
        let result = service
            .add_profile(
                user_id,
                "Alice".to_string(),
                "Bio".to_string(),
                "niche".to_string(),
                "https://example.com/avatar.png".to_string(),
                "alice_tech".to_string(),
                vec![],
                vec![],
            )
            .await;

        assert!(matches!(
            result,
            Err(ProfileServiceError::RepositoryError(_))
        ));
    }

    #[tokio::test]
    async fn test_add_profile_with_no_social_handles_and_no_rates() {
        let user_id = Uuid::new_v4();
        let profile = make_profile(user_id);

        let mut mock = MockProfileRepositoryTrait::new();
        mock.expect_create_with_details()
            .times(1)
            .returning(move |_, _, _, _, _, _, _, _| {
                Ok(ProfileWithDetails {
                    profile: Profile { ..profile.clone() },
                    social_handles: vec![],
                    rates: vec![],
                })
            });

        let service = ProfileService::new(mock);
        let result = service
            .add_profile(
                user_id,
                "Alice".to_string(),
                "Tech creator".to_string(),
                "technology".to_string(),
                "https://example.com/avatar.png".to_string(),
                "alice_tech".to_string(),
                vec![],
                vec![],
            )
            .await;

        assert!(result.is_ok());
        let details = result.unwrap();
        assert!(details.social_handles.is_empty());
        assert!(details.rates.is_empty());
    }

    #[tokio::test]
    async fn test_add_profile_with_multiple_handles_and_rates() {
        let user_id = Uuid::new_v4();
        let profile = make_profile(user_id);
        let profile_id = profile.id;

        let mut mock = MockProfileRepositoryTrait::new();
        mock.expect_create_with_details()
            .times(1)
            .returning(move |_, _, _, _, _, _, handles, rates| {
                let social_handles = handles
                    .into_iter()
                    .map(|h| SocialHandle {
                        id: Uuid::new_v4(),
                        profile_id,
                        platform: h.platform,
                        handle: h.handle,
                        url: h.url,
                        follower_count: h.follower_count,
                        updated_at: Some(Utc::now()),
                    })
                    .collect();
                let inserted_rates = rates
                    .into_iter()
                    .map(|r| Rate {
                        id: Uuid::new_v4(),
                        profile_id,
                        rate_type: r.rate_type,
                        amount: r.amount,
                    })
                    .collect();
                Ok(ProfileWithDetails {
                    profile: Profile { ..profile.clone() },
                    social_handles,
                    rates: inserted_rates,
                })
            });

        let service = ProfileService::new(mock);
        let result = service
            .add_profile(
                user_id,
                "Alice".to_string(),
                "Tech creator".to_string(),
                "technology".to_string(),
                "https://example.com/avatar.png".to_string(),
                "alice_tech".to_string(),
                vec![
                    SocialHandleInput {
                        platform: "instagram".to_string(),
                        handle: "@alice_ig".to_string(),
                        url: "https://instagram.com/alice_ig".to_string(),
                        follower_count: 1_000,
                    },
                    SocialHandleInput {
                        platform: "youtube".to_string(),
                        handle: "@alice_yt".to_string(),
                        url: "https://youtube.com/@alice_yt".to_string(),
                        follower_count: 5_000,
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
            .await;

        assert!(result.is_ok());
        let details = result.unwrap();
        assert_eq!(details.social_handles.len(), 2);
        assert_eq!(details.rates.len(), 2);
        assert_eq!(details.social_handles[0].platform, "instagram");
        assert_eq!(details.social_handles[1].platform, "youtube");
        assert_eq!(details.rates[0].rate_type, "post");
        assert_eq!(details.rates[1].rate_type, "story");
    }

    #[tokio::test]
    async fn test_get_profile_by_user_id_success() {
        let user_id = Uuid::new_v4();
        let profile = make_profile(user_id);
        let mut mock = MockProfileRepositoryTrait::new();
        mock.expect_find_by_user_id()
            .with(eq(user_id))
            .times(1)
            .return_once(move |_| Ok(Some(profile.clone())));

        let service = ProfileService::new(mock);
        let result = service.get_profile_by_user_id(user_id).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().user_id, user_id);
    }

    #[tokio::test]
    async fn test_get_profile_by_user_id_not_found() {
        let user_id = Uuid::new_v4();
        let mut mock = MockProfileRepositoryTrait::new();
        mock.expect_find_by_user_id()
            .with(eq(user_id))
            .times(1)
            .return_once(|_| Ok(None));

        let service = ProfileService::new(mock);
        let result = service.get_profile_by_user_id(user_id).await;
        assert!(matches!(result, Err(ProfileServiceError::ProfileNotFound)));
    }

    #[tokio::test]
    async fn test_update_profile_by_user_id_success() {
        let user_id = Uuid::new_v4();
        let profile = make_profile(user_id);
        let mut mock = MockProfileRepositoryTrait::new();
        mock.expect_update_by_user_id()
            .with(
                eq(user_id),
                eq("Alice".to_string()),
                eq("Bio".to_string()),
                eq("niche".to_string()),
                eq("https://example.com/avatar.png".to_string()),
                eq("alice_tech".to_string()),
            )
            .times(1)
            .return_once(move |_, _, _, _, _, _| Ok(Some(profile.clone())));

        let service = ProfileService::new(mock);
        let result = service
            .update_profile_by_user_id(
                user_id,
                "Alice".to_string(),
                "Bio".to_string(),
                "niche".to_string(),
                "https://example.com/avatar.png".to_string(),
                "alice_tech".to_string(),
            )
            .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().user_id, user_id);
    }

    #[tokio::test]
    async fn test_update_profile_by_user_id_not_found() {
        let user_id = Uuid::new_v4();
        let mut mock = MockProfileRepositoryTrait::new();
        mock.expect_update_by_user_id()
            .times(1)
            .return_once(|_, _, _, _, _, _| Ok(None));

        let service = ProfileService::new(mock);
        let result = service
            .update_profile_by_user_id(
                user_id,
                "Alice".to_string(),
                "Bio".to_string(),
                "niche".to_string(),
                "https://example.com/avatar.png".to_string(),
                "alice_tech".to_string(),
            )
            .await;
        assert!(matches!(result, Err(ProfileServiceError::ProfileNotFound)));
    }
}
