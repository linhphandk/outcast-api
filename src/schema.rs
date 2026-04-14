// @generated automatically by Diesel CLI.

diesel::table! {
    profiles (id) {
        id -> Uuid,
        user_id -> Uuid,
        name -> Text,
        bio -> Text,
        niche -> Text,
        avatar_url -> Text,
        username -> Text,
        updated_at -> Nullable<Timestamptz>,
        created_at -> Nullable<Timestamptz>,
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
    }
}

diesel::table! {
    users (id) {
        id -> Uuid,
        #[max_length = 255]
        email -> Varchar,
        #[max_length = 255]
        password -> Varchar,
    }
}

diesel::joinable!(profiles -> users (user_id));
diesel::joinable!(social_handles -> profiles (profile_id));

diesel::allow_tables_to_appear_in_same_query!(profiles, social_handles, users,);
