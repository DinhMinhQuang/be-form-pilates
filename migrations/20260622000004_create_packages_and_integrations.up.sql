CREATE TABLE course_package (
    id               uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    code             text NOT NULL UNIQUE,
    name             text NOT NULL,
    sessions         int NOT NULL CHECK (sessions > 0),
    validity_months  int NOT NULL CHECK (validity_months > 0),
    status           text NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'disabled'))
);

CREATE TABLE package_class_type (
    package_id    uuid NOT NULL REFERENCES course_package(id) ON DELETE CASCADE,
    class_type_id uuid NOT NULL REFERENCES class_type(id) ON DELETE CASCADE,
    PRIMARY KEY (package_id, class_type_id)
);

CREATE TABLE haravan_product_mapping (
    id                  uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    haravan_product_id  text,
    haravan_variant_id  text NOT NULL UNIQUE,
    package_id          uuid NOT NULL REFERENCES course_package(id),
    branch_id           uuid REFERENCES branch(id),
    active              boolean NOT NULL DEFAULT true
);

CREATE TABLE integration_event (
    id                 uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    provider           text NOT NULL,
    external_event_id  text NOT NULL,
    event_type         text NOT NULL,
    payload            jsonb NOT NULL,
    signature_valid    boolean NOT NULL,
    status             text NOT NULL DEFAULT 'received' CHECK (status IN ('received', 'processing', 'processed', 'failed')),
    attempts           int NOT NULL DEFAULT 0,
    error_message      text,
    received_at        timestamptz NOT NULL DEFAULT now(),
    processed_at       timestamptz,
    UNIQUE (provider, external_event_id)
);

CREATE TABLE customer_order (
    id                 uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    provider           text NOT NULL,
    external_order_id  text NOT NULL,
    student_id         uuid NOT NULL REFERENCES app_user(id),
    financial_status   text NOT NULL,
    paid_at            timestamptz,
    raw_payload        jsonb NOT NULL,
    created_at         timestamptz NOT NULL DEFAULT now(),
    UNIQUE (provider, external_order_id)
);

CREATE TABLE order_item (
    id                   uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    order_id             uuid NOT NULL REFERENCES customer_order(id) ON DELETE CASCADE,
    external_variant_id  text NOT NULL,
    package_id           uuid REFERENCES course_package(id),
    quantity             int NOT NULL CHECK (quantity > 0)
);
