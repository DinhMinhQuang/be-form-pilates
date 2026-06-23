CREATE TABLE class_session (
    id             uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    branch_id      uuid NOT NULL REFERENCES branch(id),
    class_type_id  uuid NOT NULL REFERENCES class_type(id),
    trainer_id     uuid REFERENCES app_user(id),
    start_at       timestamptz NOT NULL,
    end_at         timestamptz NOT NULL,
    capacity       int NOT NULL CHECK (capacity BETWEEN 1 AND 6),
    booked_count   int NOT NULL DEFAULT 0 CHECK (booked_count >= 0 AND booked_count <= capacity),
    status         text NOT NULL DEFAULT 'scheduled' CHECK (status IN ('scheduled', 'cancelled', 'completed')),
    created_by     uuid REFERENCES app_user(id),
    created_at     timestamptz NOT NULL DEFAULT now(),
    CHECK (end_at > start_at)
);
CREATE INDEX idx_session_schedule ON class_session (branch_id, start_at, status);
CREATE INDEX idx_session_trainer ON class_session (trainer_id, start_at);
