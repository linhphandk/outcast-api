use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use cookie::time::Duration;

use super::super::usecase::session_service::{
    REFRESH_COOKIE_MAX_AGE_SECS, TOKEN_COOKIE_MAX_AGE_SECS,
};

pub const TOKEN_COOKIE_NAME: &str = "token";
pub const REFRESH_TOKEN_COOKIE_NAME: &str = "refresh_token";

/// Sets `token` and `refresh_token` cookies with the canonical security attributes.
///
/// * `token`         — HttpOnly, SameSite=Strict, Path=/, Secure (non-debug builds)
/// * `refresh_token` — HttpOnly, SameSite=Strict, Path=/auth/refresh, Secure (non-debug builds)
pub fn set_auth_cookies(jar: CookieJar, access: String, refresh: String) -> CookieJar {
    let secure = false;

    let token_cookie = Cookie::build((TOKEN_COOKIE_NAME, access))
        .http_only(true)
        .same_site(SameSite::Lax)
        .path("/")
        .secure(secure)
        .max_age(Duration::seconds(TOKEN_COOKIE_MAX_AGE_SECS));

    let refresh_cookie = Cookie::build((REFRESH_TOKEN_COOKIE_NAME, refresh))
        .http_only(true)
        .same_site(SameSite::Lax)
        .path("/auth/refresh")
        .secure(secure)
        .max_age(Duration::seconds(REFRESH_COOKIE_MAX_AGE_SECS));

    jar.add(token_cookie).add(refresh_cookie)
}

/// Clears `token` and `refresh_token` cookies.
///
/// Uses `add()` with `Max-Age=0` rather than `remove()` so that the
/// `Set-Cookie` removal headers are **always** emitted in the response,
/// even when the client did not include those cookies in the request.
pub fn clear_auth_cookies(jar: CookieJar) -> CookieJar {
    let secure = false;

    let token_cookie = Cookie::build((TOKEN_COOKIE_NAME, ""))
        .http_only(true)
        .same_site(SameSite::Lax)
        .path("/")
        .secure(secure)
        .max_age(Duration::seconds(0));

    let refresh_cookie = Cookie::build((REFRESH_TOKEN_COOKIE_NAME, ""))
        .http_only(true)
        .same_site(SameSite::Lax)
        .path("/auth/refresh")
        .secure(secure)
        .max_age(Duration::seconds(0));

    jar.add(token_cookie).add(refresh_cookie)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find_cookie<'a>(jar: &'a CookieJar, name: &str) -> Option<Cookie<'a>> {
        jar.get(name).cloned()
    }

    #[test]
    fn set_auth_cookies_adds_token_cookie() {
        let jar = CookieJar::new();
        let jar = set_auth_cookies(jar, "access_token_val".into(), "refresh_token_val".into());

        let cookie = find_cookie(&jar, TOKEN_COOKIE_NAME).expect("token cookie missing");
        assert_eq!(cookie.value(), "access_token_val");
        assert_eq!(cookie.http_only(), Some(true));
        assert_eq!(cookie.same_site(), Some(SameSite::Lax));
        assert_eq!(cookie.path(), Some("/"));
        assert_eq!(
            cookie.max_age(),
            Some(Duration::seconds(TOKEN_COOKIE_MAX_AGE_SECS))
        );
    }

    #[test]
    fn set_auth_cookies_adds_refresh_cookie() {
        let jar = CookieJar::new();
        let jar = set_auth_cookies(jar, "access_token_val".into(), "refresh_token_val".into());

        let cookie = find_cookie(&jar, REFRESH_TOKEN_COOKIE_NAME).expect("refresh_token cookie missing");
        assert_eq!(cookie.value(), "refresh_token_val");
        assert_eq!(cookie.http_only(), Some(true));
        assert_eq!(cookie.same_site(), Some(SameSite::Lax));
        assert_eq!(cookie.path(), Some("/auth/refresh"));
        assert_eq!(
            cookie.max_age(),
            Some(Duration::seconds(REFRESH_COOKIE_MAX_AGE_SECS))
        );
    }

    #[test]
    fn clear_auth_cookies_removes_both_cookies() {
        let jar = CookieJar::new();
        let jar = set_auth_cookies(jar, "a".into(), "r".into());

        assert!(jar.get(TOKEN_COOKIE_NAME).is_some());
        assert!(jar.get(REFRESH_TOKEN_COOKIE_NAME).is_some());

        let jar = clear_auth_cookies(jar);

        // Both cookies should be set to empty with Max-Age=0, telling the
        // browser to delete them.
        let token = jar.get(TOKEN_COOKIE_NAME).expect("token cookie must be present for removal");
        assert_eq!(token.value(), "");
        assert_eq!(token.max_age(), Some(Duration::seconds(0)));

        let refresh = jar.get(REFRESH_TOKEN_COOKIE_NAME).expect("refresh_token cookie must be present for removal");
        assert_eq!(refresh.value(), "");
        assert_eq!(refresh.max_age(), Some(Duration::seconds(0)));
    }
}
