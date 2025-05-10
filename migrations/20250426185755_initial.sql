CREATE TABLE resources (
    id VARCHAR(36) PRIMARY KEY, -- uuid
    name VARCHAR(255) NOT NULL,
    namespace VARCHAR(255) NOT NULL,
    resource_type VARCHAR(255) NOT NULL,
    manifest JSONB NOT NULl,
    hash VARCHAR(255) NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX triple_idx ON resources(resource_type, name, namespace);
