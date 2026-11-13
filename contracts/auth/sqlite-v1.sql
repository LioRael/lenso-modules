CREATE TABLE IF NOT EXISTS lenso_auth_migrations (version INTEGER PRIMARY KEY);
CREATE TABLE IF NOT EXISTS lenso_auth_users (
 realm TEXT NOT NULL, id TEXT NOT NULL, name TEXT NOT NULL, password_hash TEXT NOT NULL,
 PRIMARY KEY (realm, id), UNIQUE (realm, name)
);
CREATE TABLE IF NOT EXISTS lenso_auth_sessions (
 realm TEXT NOT NULL, token_hash TEXT NOT NULL, subject TEXT NOT NULL, expires_ms INTEGER NOT NULL CHECK (expires_ms >= 0),
 PRIMARY KEY (realm, token_hash), FOREIGN KEY (realm, subject) REFERENCES lenso_auth_users(realm, id)
);
CREATE INDEX IF NOT EXISTS lenso_auth_expiry ON lenso_auth_sessions(realm, expires_ms);
INSERT INTO lenso_auth_migrations(version) VALUES (1) ON CONFLICT DO NOTHING;
