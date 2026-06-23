CREATE TABLE app_user (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    role        text NOT NULL CHECK (role IN ('student', 'trainer', 'admin')),
    email       text,
    phone       text,
    full_name   text NOT NULL DEFAULT '',
    status      text NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'disabled')),
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    CHECK (email IS NOT NULL OR phone IS NOT NULL)
);
CREATE UNIQUE INDEX uniq_user_email ON app_user (lower(email)) WHERE email IS NOT NULL;
CREATE UNIQUE INDEX uniq_user_phone ON app_user (phone) WHERE phone IS NOT NULL;

CREATE TABLE student_profile (
    user_id              uuid PRIMARY KEY REFERENCES app_user(id) ON DELETE CASCADE,
    haravan_customer_id  text UNIQUE,
    notes                text
);

CREATE TABLE staff_credential (
    user_id        uuid PRIMARY KEY REFERENCES app_user(id) ON DELETE CASCADE,
    password_hash  text NOT NULL,
    last_login_at  timestamptz
);

CREATE TABLE auth_session (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     uuid NOT NULL REFERENCES app_user(id) ON DELETE CASCADE,
    token_hash  text NOT NULL UNIQUE,
    expires_at  timestamptz NOT NULL,
    revoked_at  timestamptz,
    created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX idx_auth_session_user ON auth_session (user_id, expires_at);

CREATE TABLE magic_link (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    student_id  uuid NOT NULL REFERENCES app_user(id) ON DELETE CASCADE,
    token_hash  text NOT NULL UNIQUE,
    expires_at  timestamptz NOT NULL,
    used_at     timestamptz,
    created_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE email_outbox (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    recipient   text NOT NULL,
    template    text NOT NULL,
    payload     jsonb NOT NULL,
    status      text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'sent', 'failed')),
    attempts    int NOT NULL DEFAULT 0,
    created_at  timestamptz NOT NULL DEFAULT now(),
    sent_at     timestamptz
);
