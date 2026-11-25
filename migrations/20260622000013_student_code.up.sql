CREATE SEQUENCE student_code_seq;
ALTER TABLE app_user ADD COLUMN student_code text UNIQUE;

CREATE FUNCTION assign_student_code() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE code_number bigint;
BEGIN
    IF TG_OP = 'UPDATE' AND OLD.student_code IS NOT NULL
       AND NEW.student_code IS DISTINCT FROM OLD.student_code THEN
        RAISE EXCEPTION 'student_code cannot be changed';
    END IF;
    IF NEW.role = 'student' AND NEW.student_code IS NULL THEN
        code_number := nextval('student_code_seq');
        NEW.student_code := 'FORM-' || lpad(code_number::text, greatest(6, length(code_number::text)), '0');
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER assign_student_code BEFORE INSERT OR UPDATE ON app_user
FOR EACH ROW EXECUTE FUNCTION assign_student_code();

-- Assign existing students deterministically, oldest first.
DO $$
DECLARE student record;
BEGIN
    FOR student IN SELECT id FROM app_user WHERE role = 'student' ORDER BY created_at, id LOOP
        UPDATE app_user SET student_code = NULL WHERE id = student.id;
    END LOOP;
END;
$$;
ALTER TABLE app_user ADD CONSTRAINT student_code_required
CHECK (role <> 'student' OR student_code IS NOT NULL);
