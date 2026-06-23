WITH admin_ins AS (
    INSERT INTO app_user (role, email, full_name)
    VALUES ('admin', 'admin@formpilates.vn', 'Admin')
    RETURNING id
)
INSERT INTO staff_credential (user_id, password_hash)
SELECT id, '$argon2id$v=19$m=19456,t=2,p=1$TBy+6IKzPiefZJ8N6vYYog$kcTA2Lqip7jkfGbi8+1re3Gaay7DN6Yo90B4L4lBKdI' FROM admin_ins;

WITH trainer1_ins AS (
    INSERT INTO app_user (role, email, phone, full_name)
    VALUES ('trainer', 'trainer.linh@formpilates.vn', '0901000001', 'Nguyễn Thị Linh')
    RETURNING id
)
INSERT INTO staff_credential (user_id, password_hash)
SELECT id, '$argon2id$v=19$m=19456,t=2,p=1$aer0zmYcLFF/Oe+BagVtng$l2aMPPmYRf3D5DXzCcHWZ3/RghTM+EnNQkCnuF+yJI8' FROM trainer1_ins;

WITH trainer2_ins AS (
    INSERT INTO app_user (role, email, phone, full_name)
    VALUES ('trainer', 'trainer.mai@formpilates.vn', '0901000002', 'Trần Thị Mai')
    RETURNING id
)
INSERT INTO staff_credential (user_id, password_hash)
SELECT id, '$argon2id$v=19$m=19456,t=2,p=1$aer0zmYcLFF/Oe+BagVtng$l2aMPPmYRf3D5DXzCcHWZ3/RghTM+EnNQkCnuF+yJI8' FROM trainer2_ins;

WITH student1_ins AS (
    INSERT INTO app_user (role, email, phone, full_name)
    VALUES ('student', 'student.an@gmail.com', '0909000001', 'Lê Thị An')
    RETURNING id
)
INSERT INTO student_profile (user_id)
SELECT id FROM student1_ins;

WITH student2_ins AS (
    INSERT INTO app_user (role, phone, full_name)
    VALUES ('student', '0909000002', 'Phạm Văn Bình')
    RETURNING id
)
INSERT INTO student_profile (user_id)
SELECT id FROM student2_ins;
