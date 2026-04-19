pub mod config;
pub mod instagram;
pub mod schema;
pub mod session;
pub mod tiktok;
pub mod user;

use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::user::http::user_controller::create_user,
        crate::user::http::user_controller::login_user,
        crate::user::http::user_controller::get_me,
        crate::user::http::profile_controller::create_my_profile,
        crate::user::http::profile_controller::get_my_profile,
        crate::user::http::profile_controller::get_platforms,
        crate::user::http::profile_controller::update_my_profile,
        crate::instagram::http::instagram_authorize,
        crate::instagram::http::instagram_callback,
        crate::instagram::http::disconnect_instagram,
        crate::instagram::http::refresh_instagram
    ),
    components(
        schemas(
            crate::user::http::user_controller::CreateUserReq,
            crate::user::http::user_controller::CreateUserRes,
            crate::user::http::user_controller::LoginUserReq,
            crate::user::http::user_controller::MeRes,
            crate::user::http::profile_controller::CreatorProfileRes,
            crate::user::http::profile_controller::CreatorProfileWithDetailsRes,
            crate::user::http::profile_controller::SocialHandleRes,
            crate::user::http::profile_controller::RateRes,
            crate::user::http::profile_controller::CreateCreatorProfileReq,
            crate::user::http::profile_controller::SocialHandleInputReq,
            crate::user::http::profile_controller::RateInputReq,
            crate::user::http::profile_controller::UpdateCreatorProfileReq,
            crate::instagram::http::InstagramCallbackQuery,
            crate::instagram::http::InstagramSocialHandleRes
        )
    ),
    tags(
        (name = "Users", description = "User management endpoints"),
        (name = "Profiles", description = "Creator profile endpoints"),
        (name = "Instagram OAuth", description = "Instagram OAuth endpoints")
    ),
    info(
        title = "Outcast API",
        version = "1.0.0",
        description = "Outcast API documentation",
        license(name = "MIT", url = "https://opensource.org/licenses/MIT")
    )
)]
pub struct ApiDoc;
