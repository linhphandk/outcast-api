// @generated automatically by Diesel CLI.

diesel::table! {
    oauth_tokens (id) {
        id -> Uuid,
        profile_id -> Uuid,
        provider -> Text,
        access_token -> Text,
        refresh_token -> Nullable<Text>,
        expires_at -> Nullable<Timestamptz>,
        provider_user_id -> Text,
        scopes -> Text,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    profiles (id) {
        id -> Uuid,
        user_id -> Uuid,
        name -> Text,
        bio -> Text,
        niche -> Text,
        avatar_url -> Text,
        username -> Citext,
        updated_at -> Nullable<Timestamptz>,
        created_at -> Nullable<Timestamptz>,
    }
}

diesel::table! {
    rates (id) {
        id -> Uuid,
        profile_id -> Uuid,
        #[sql_name = "type"]
        type_ -> Text,
        amount -> Numeric,
    }
}

diesel::table! {
    sessions (id) {
        id -> Uuid,
        user_id -> Uuid,
        #[max_length = 512]
        refresh_token -> Varchar,
        user_agent -> Nullable<Text>,
        #[max_length = 45]
        ip_address -> Nullable<Varchar>,
        expires_at -> Timestamp,
        revoked_at -> Nullable<Timestamp>,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

diesel::table! {
    social_handles (id) {
        id -> Uuid,
        profile_id -> Uuid,
        platform -> Text,
        handle -> Text,
        url -> Text,
        follower_count -> Int4,
        updated_at -> Nullable<Timestamptz>,
        engagement_rate -> Numeric,
        last_synced_at -> Nullable<Timestamptz>,
    }
}

diesel::table! {
    users (id) {
        id -> Uuid,
        #[max_length = 255]
        email -> Varchar,
        #[max_length = 255]
        password -> Varchar,
        #[max_length = 512]
        avatar_url -> Nullable<Varchar>,
    }
}

diesel::joinable!(oauth_tokens -> profiles (profile_id));
diesel::joinable!(profiles -> users (user_id));
diesel::joinable!(rates -> profiles (profile_id));
diesel::joinable!(sessions -> users (user_id));
diesel::joinable!(social_handles -> profiles (profile_id));

diesel::allow_tables_to_appear_in_same_query!(
    oauth_tokens,
    profiles,
    rates,
    sessions,
    social_handles,
    users,
);
