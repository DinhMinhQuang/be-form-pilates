WITH dev_ins AS (
    INSERT INTO app_user (role, email, full_name)
    VALUES ('admin', 'minhquang.deverloper@gmail.com', 'Minh Quang')
    ON CONFLICT (lower(email)) WHERE email IS NOT NULL DO UPDATE
        SET full_name = EXCLUDED.full_name
    RETURNING id
)
INSERT INTO staff_credential (user_id, password_hash)
SELECT id, '$argon2id$v=19$m=19456,t=2,p=1$QKLNDCy1UERRh7DNPXnNJw$xEKPOnAufbZL1FX8+rcprdNz8xwjB28HTiorySJwKAs' FROM dev_ins
ON CONFLICT (user_id) DO UPDATE SET password_hash = EXCLUDED.password_hash;
