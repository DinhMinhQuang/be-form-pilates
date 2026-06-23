CREATE TABLE credit_lot (
    id                  uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    student_id          uuid NOT NULL REFERENCES app_user(id),
    package_id          uuid NOT NULL REFERENCES course_package(id),
    order_item_id       uuid REFERENCES order_item(id),
    sessions_total      int NOT NULL CHECK (sessions_total > 0),
    sessions_remaining  int NOT NULL CHECK (sessions_remaining >= 0),
    activated_at        timestamptz NOT NULL,
    expires_at          timestamptz NOT NULL,
    status              text NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'frozen', 'void')),
    created_at          timestamptz NOT NULL DEFAULT now(),
    CHECK (sessions_remaining <= sessions_total),
    CHECK (expires_at > activated_at)
);
CREATE INDEX idx_credit_lot_pick ON credit_lot (student_id, expires_at)
    WHERE sessions_remaining > 0 AND status = 'active';

CREATE TABLE booking (
    id                    uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    session_id            uuid NOT NULL REFERENCES class_session(id),
    student_id            uuid NOT NULL REFERENCES app_user(id),
    credit_lot_id         uuid NOT NULL REFERENCES credit_lot(id),
    status                text NOT NULL CHECK (status IN ('booked', 'cancelled_refunded', 'attended', 'no_show')),
    booked_by             uuid NOT NULL REFERENCES app_user(id),
    channel               text NOT NULL CHECK (channel IN ('student', 'admin')),
    booked_at             timestamptz NOT NULL DEFAULT now(),
    cancelled_at          timestamptz,
    cancellation_reason   text,
    attended_at           timestamptz,
    attendance_marked_by  uuid REFERENCES app_user(id)
);
CREATE UNIQUE INDEX uniq_active_booking ON booking (session_id, student_id)
    WHERE status IN ('booked', 'attended', 'no_show');
CREATE INDEX idx_booking_student ON booking (student_id, booked_at DESC);

CREATE TABLE credit_ledger (
    id                    uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    student_id            uuid NOT NULL REFERENCES app_user(id),
    lot_id                uuid NOT NULL REFERENCES credit_lot(id),
    booking_id            uuid REFERENCES booking(id),
    integration_event_id  uuid REFERENCES integration_event(id),
    delta                 int NOT NULL CHECK (delta <> 0),
    balance_after         int NOT NULL CHECK (balance_after >= 0),
    reason                text NOT NULL,
    actor_id              uuid REFERENCES app_user(id),
    metadata              jsonb NOT NULL DEFAULT '{}'::jsonb,
    created_at            timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX idx_credit_ledger_student ON credit_ledger (student_id, created_at DESC);

CREATE TABLE credit_expiry_change (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    credit_lot_id   uuid NOT NULL REFERENCES credit_lot(id),
    old_expires_at  timestamptz NOT NULL,
    new_expires_at  timestamptz NOT NULL,
    reason          text NOT NULL,
    changed_by      uuid NOT NULL REFERENCES app_user(id),
    changed_at      timestamptz NOT NULL DEFAULT now()
);
