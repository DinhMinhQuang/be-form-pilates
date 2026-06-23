INSERT INTO branch (code, name, address) VALUES
('TML', 'FORM Thạnh Mỹ Lợi', '18 đường 75, phường Thạnh Mỹ Lợi, Cát Lái'),
('TD', 'FORM Thảo Điền', '216/26 Nguyễn Văn Hưởng, phường An Khánh');

INSERT INTO class_type (code, name, category, level, default_capacity) VALUES
('stott-reformer', 'STOTT Pilates', 'group_reformer', 'all_levels', 6),
('feel-good-full-body', 'Feel Good Full Body', 'group_reformer', 'all_levels', 6),
('functional-strength', 'Functional Strength Pilates', 'group_reformer', 'intermediate', 6),
('level-up', 'Level Up', 'group_reformer', 'intermediate_advanced', 6),
('cardio-tramp', 'Cardio Tramp Reformer', 'group_reformer', 'all_levels', 6),
('stott-mat', 'STOTT Pilates (Mat)', 'group_mat', 'all_levels', 6),
('reformer-on-mat', 'Reformer on the MAT', 'group_mat', 'all_levels', 6),
('private', 'Private Session', 'private', 'all_levels', 1),
('duo', 'Duo Session', 'duo', 'all_levels', 2);

INSERT INTO branch_class_type (branch_id, class_type_id)
SELECT b.id, ct.id FROM branch b CROSS JOIN class_type ct
WHERE b.code = 'TML' AND ct.code IN
('stott-reformer', 'feel-good-full-body', 'functional-strength', 'level-up', 'cardio-tramp', 'private', 'duo');

INSERT INTO branch_class_type (branch_id, class_type_id)
SELECT b.id, ct.id FROM branch b CROSS JOIN class_type ct
WHERE b.code = 'TD' AND ct.code IN ('stott-mat', 'reformer-on-mat', 'private', 'duo');

INSERT INTO course_package (code, name, sessions, validity_months) VALUES
('3-sessions', 'Gói 3 buổi', 3, 1),
('10-sessions', 'Gói 10 buổi', 10, 2),
('20-sessions', 'Gói 20 buổi', 20, 5),
('40-sessions', 'Gói 40 buổi', 40, 12),
('60-sessions', 'Gói 60 buổi', 60, 18);
