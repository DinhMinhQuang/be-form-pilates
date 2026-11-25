DROP TRIGGER assign_student_code ON app_user;
DROP FUNCTION assign_student_code();
ALTER TABLE app_user DROP CONSTRAINT student_code_required;
ALTER TABLE app_user DROP COLUMN student_code;
DROP SEQUENCE student_code_seq;
