-- Add migration script here
CREATE TABLE peers
(
    id         INTEGER PRIMARY KEY AUTOINCREMENT, -- uuid
    node_name  VARCHAR(255) NOT NULL UNIQUE,
    ip_address VARCHAR(255) NOT NULL UNIQUE,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
