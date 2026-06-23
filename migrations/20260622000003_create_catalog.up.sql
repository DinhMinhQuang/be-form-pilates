CREATE TABLE branch (
    id        uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    code      text NOT NULL UNIQUE,
    name      text NOT NULL,
    address   text NOT NULL,
    timezone  text NOT NULL DEFAULT 'Asia/Ho_Chi_Minh',
    status    text NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'disabled'))
);

CREATE TABLE class_type (
    id                uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    code              text NOT NULL UNIQUE,
    name              text NOT NULL,
    description       text NOT NULL DEFAULT '',
    category          text NOT NULL CHECK (category IN ('group_reformer', 'group_mat', 'private', 'duo')),
    level             text NOT NULL DEFAULT 'all_levels',
    default_capacity  int NOT NULL CHECK (default_capacity BETWEEN 1 AND 6),
    status            text NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'disabled'))
);

CREATE TABLE branch_class_type (
    branch_id         uuid NOT NULL REFERENCES branch(id) ON DELETE CASCADE,
    class_type_id     uuid NOT NULL REFERENCES class_type(id) ON DELETE CASCADE,
    enabled           boolean NOT NULL DEFAULT true,
    capacity_override int CHECK (capacity_override BETWEEN 1 AND 6),
    PRIMARY KEY (branch_id, class_type_id)
);
