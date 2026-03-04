-- Generate 1M rows for performance testing in PostgreSQL
-- Run this against your testdb

-- Create a test table with several column types
CREATE TABLE IF NOT EXISTS perf_test (
    id SERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    email TEXT NOT NULL,
    age INTEGER NOT NULL,
    salary NUMERIC(10,2),
    department TEXT,
    is_active BOOLEAN DEFAULT true,
    created_at TIMESTAMP DEFAULT NOW()
);

-- Truncate if re-running
TRUNCATE perf_test RESTART IDENTITY;

-- Insert 1M rows using generate_series
-- This takes ~10-30 seconds depending on your machine
INSERT INTO perf_test (name, email, age, salary, department, is_active, created_at)
SELECT
    'User_' || gs AS name,
    'user' || gs || '@example.com' AS email,
    18 + (gs % 50) AS age,
    30000 + (gs % 100000)::numeric / 100 AS salary,
    CASE (gs % 6)
        WHEN 0 THEN 'Engineering'
        WHEN 1 THEN 'Marketing'
        WHEN 2 THEN 'Sales'
        WHEN 3 THEN 'Support'
        WHEN 4 THEN 'Finance'
        WHEN 5 THEN 'HR'
    END AS department,
    (gs % 5 != 0) AS is_active,
    NOW() - (gs || ' seconds')::interval AS created_at
FROM generate_series(1, 1000000) AS gs;

-- Verify row count
SELECT COUNT(*) AS total_rows FROM perf_test;
