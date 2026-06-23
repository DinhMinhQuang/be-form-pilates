DELETE FROM course_package
WHERE code IN ('3-sessions', '10-sessions', '20-sessions', '40-sessions', '60-sessions');

DELETE FROM branch_class_type;

DELETE FROM class_type
WHERE code IN (
    'stott-reformer',
    'feel-good-full-body',
    'functional-strength',
    'level-up',
    'cardio-tramp',
    'stott-mat',
    'reformer-on-mat',
    'private',
    'duo'
);

DELETE FROM branch WHERE code IN ('TML', 'TD');
