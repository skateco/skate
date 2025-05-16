-- Add migration script here
ALTER TABLE resources
ADD COLUMN generation integer NOT NULL DEFAULT 1;
