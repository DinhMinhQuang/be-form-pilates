-- Thêm học viên mẫu
WITH s1 AS (
    INSERT INTO app_user (role, email, phone, full_name)
    VALUES ('student', 'linh.nguyen@gmail.com', '0912000001', 'Nguyễn Thị Linh')
    RETURNING id
)
INSERT INTO student_profile (user_id) SELECT id FROM s1;

WITH s2 AS (
    INSERT INTO app_user (role, email, phone, full_name)
    VALUES ('student', 'mai.tran@gmail.com', '0912000002', 'Trần Thị Mai')
    RETURNING id
)
INSERT INTO student_profile (user_id) SELECT id FROM s2;

WITH s3 AS (
    INSERT INTO app_user (role, phone, full_name)
    VALUES ('student', '0912000003', 'Phạm Thị Hoa')
    RETURNING id
)
INSERT INTO student_profile (user_id) SELECT id FROM s3;

-- Tạo lịch học mẫu (chỉ chạy được sau khi sync Haravan tạo branch)
-- Dùng DO block để bỏ qua nếu branch chưa có
DO $$
DECLARE
    v_branch_id   uuid;
    v_trainer_id  uuid;
    v_ct_stott    uuid;
    v_ct_feelgood uuid;
    v_ct_mat      uuid;
    v_ct_private  uuid;
    v_ct_duo      uuid;
BEGIN
    SELECT id INTO v_trainer_id FROM app_user WHERE role = 'trainer' AND status = 'active' LIMIT 1;
    SELECT id INTO v_ct_stott    FROM class_type WHERE code = 'stott-reformer';
    SELECT id INTO v_ct_feelgood FROM class_type WHERE code = 'feel-good-full-body';
    SELECT id INTO v_ct_mat      FROM class_type WHERE code = 'stott-mat';
    SELECT id INTO v_ct_private  FROM class_type WHERE code = 'private';
    SELECT id INTO v_ct_duo      FROM class_type WHERE code = 'duo';

    -- Lấy branch đầu tiên có sẵn
    SELECT id INTO v_branch_id FROM branch WHERE status = 'active' LIMIT 1;

    IF v_branch_id IS NULL THEN
        RAISE NOTICE 'No branch found — skipping class session seed. Run Haravan sync first.';
        RETURN;
    END IF;

    -- Insert branch_class_type cho branch này (nếu chưa có)
    INSERT INTO branch_class_type (branch_id, class_type_id) VALUES
        (v_branch_id, v_ct_stott),
        (v_branch_id, v_ct_feelgood),
        (v_branch_id, v_ct_mat),
        (v_branch_id, v_ct_private),
        (v_branch_id, v_ct_duo)
    ON CONFLICT DO NOTHING;

    -- Tạo lịch học tuần này
    INSERT INTO class_session (branch_id, class_type_id, trainer_id, start_at, end_at, capacity) VALUES
        (v_branch_id, v_ct_stott,    v_trainer_id, now() + interval '1 day' + interval '7 hour',  now() + interval '1 day' + interval '8 hour',  6),
        (v_branch_id, v_ct_feelgood, v_trainer_id, now() + interval '1 day' + interval '9 hour',  now() + interval '1 day' + interval '10 hour', 6),
        (v_branch_id, v_ct_stott,    v_trainer_id, now() + interval '2 day' + interval '7 hour',  now() + interval '2 day' + interval '8 hour',  6),
        (v_branch_id, v_ct_mat,      v_trainer_id, now() + interval '2 day' + interval '9 hour',  now() + interval '2 day' + interval '10 hour', 6),
        (v_branch_id, v_ct_feelgood, v_trainer_id, now() + interval '3 day' + interval '7 hour',  now() + interval '3 day' + interval '8 hour',  6),
        (v_branch_id, v_ct_private,  v_trainer_id, now() + interval '3 day' + interval '10 hour', now() + interval '3 day' + interval '11 hour', 1),
        (v_branch_id, v_ct_duo,      v_trainer_id, now() + interval '4 day' + interval '8 hour',  now() + interval '4 day' + interval '9 hour',  2),
        (v_branch_id, v_ct_stott,    v_trainer_id, now() + interval '5 day' + interval '7 hour',  now() + interval '5 day' + interval '8 hour',  6);
END $$;
