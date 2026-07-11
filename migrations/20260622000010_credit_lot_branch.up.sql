ALTER TABLE credit_lot ADD COLUMN branch_id uuid REFERENCES branch(id);
