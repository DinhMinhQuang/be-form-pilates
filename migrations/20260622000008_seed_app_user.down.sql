DELETE FROM app_user
WHERE email IN (
    'admin@formpilates.vn',
    'trainer.linh@formpilates.vn',
    'trainer.mai@formpilates.vn',
    'student.an@gmail.com'
)
OR phone IN ('0909000002');
