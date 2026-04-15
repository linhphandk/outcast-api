use axum_extra::extract::CookieJar;
use cookie::{Cookie, SameSite};
use time::Duration;

use crate::session::usecase::session_service::{
    REFRESH_COOKIE_MAX_AGE_SECS, TOKEN_COOKIE_MAX_AGE_SECS,
};

/// Add `token` and `refresh_token` cookies to `jar` with the canonical attributes.
///
/// - `token`: HttpOnly, SameSite=Strict, Path=/, Secure=`secure`, Max-Age=900 s
/// - `refresh_token`: HttpOnly, SameSite=Strict, Path=/auth/refresh, Secure=`secure`, Max-Age=604800 s
pub fn set_auth_cookies(
    jar: CookieJar,
    access_token: String,
    refresh_token: String,
    secure: bool,
) -> CookieJar {
    let token_cookie = Cookie::build(("token", access_token))
        .http_only(true)
        .same_site(SameSite::Strict)
        .path("/")
        .secure(secure)
        .max_age(Duration::seconds(TOKEN_COOKIE_MAX_AGE_SECS))
        .build();

    let refresh_cookie = Cookie::build(("refresh_token", refresh_token))
        .http_only(true)
        .same_site(SameSite::Strict)
        .path("/auth/refresh")
        .secure(secure)
        .max_age(Duration::seconds(REFRESH_COOKIE_MAX_AGE_SECS))
        .build();

    jar.add(token_cookie).add(refresh_cookie)
}

/// Remove `token` and `refresh_token` cookies (adds expired Set-Cookie headers).
pub fn clear_auth_cookies(jar: CookieJar) -> CookieJar {
    jar.remove(Cookie::build(("token", "")).path("/").build())
        .remove(Cookie::build(("refresh_token", "")).path("/auth/refresh").build())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_auth_cookies_attributes() {
        let jar = CookieJar::default();
        let jar = set_auth_cookies(jar, "access123".to_owned(), "refresh456".to_owned(), true);

        let token = jar.get("token").expect("token cookie should be present");
        assert!(token.http_only());
        assert_eq!(token.same_site(), Some(SameSite::Strict));
        assert_eq!(token.path(), Some("/"));
        assert_eq!(token.secure(), Some(true));
        assert_eq!(
            token.max_age(),
            Some(Duration::seconds(TOKEN_COOKIE_MAX_AGE_SECS))
        );

        let refresh = jar
            .get("refresh_token")
            .expect("refresh_token cookie should be present");
        assert!(refresh.http_only());
        assert_eq!(refresh.same_site(), Some(SameSite::Strict));
        assert_eq!(refresh.path(), Some("/auth/refresh"));
        assert_eq!(refresh.secure(), Some(true));
        assert_eq!(
            refresh.max_age(),
            Some(Duration::seconds(REFRESH_COOKIE_MAX_AGE_SECS))
        );
    }

    #[test]
    fn test_set_auth_cookies_insecure_flag() {
        let jar = CookieJar::default();
        let jar = set_auth_cookies(jar, "tok".to_owned(), "ref".to_owned(), false);

        let token = jar.get("token").expect("token cookie present");
        assert_eq!(token.secure(), Some(false));

        let refresh = jar.get("refresh_token").expect("refresh_token cookie present");
        assert_eq!(refresh.secure(), Some(false));
    }

    #[test]
    fn test_clear_auth_cookies() {
        let jar = CookieJar::default();
        let jar = set_auth_cookies(jar, "tok".to_owned(), "ref".to_owned(), false);

        // Sanity: cookies are present before clearing.
        assert!(jar.get("token").is_some());
        assert!(jar.get("refresh_token").is_some());

        let jar = clear_auth_cookies(jar);

        // After clearing, get() should return None.
        assert!(jar.get("token").is_none());
        assert!(jar.get("refresh_token").is_none());
    }
}
