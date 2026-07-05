DELETE FROM class_session WHERE created_by IS NULL;
DELETE FROM branch_class_type;
DELETE FROM app_user WHERE email IN ('linh.nguyen@gmail.com', 'mai.tran@gmail.com') OR phone = '0912000003';
