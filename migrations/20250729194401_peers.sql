-- Add migration script here
CREATE TABLE peers
(
    id          INTEGER PRIMARY KEY AUTOINCREMENT, -- uuid
    node_name   VARCHAR(255) NOT NULL UNIQUE,
    host        VARCHAR(255) NOT NULL UNIQUE,
    subnet_cidr VARCHAR(255) NOT NULL,
    created_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
