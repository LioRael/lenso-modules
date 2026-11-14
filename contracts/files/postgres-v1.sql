CREATE TABLE IF NOT EXISTS lenso_files_migrations(version INTEGER PRIMARY KEY CHECK(version = 1));
INSERT INTO lenso_files_migrations(version) VALUES(1) ON CONFLICT DO NOTHING;
CREATE TABLE IF NOT EXISTS lenso_files (
  realm TEXT NOT NULL, id TEXT NOT NULL, owner TEXT NOT NULL, name TEXT NOT NULL,
  size BIGINT NOT NULL CHECK(size >= 0), sha256 TEXT NOT NULL, created_ms BIGINT NOT NULL,
  state TEXT NOT NULL CHECK(state IN ('pending','ready','deleting')), PRIMARY KEY(realm,id)
);
CREATE INDEX IF NOT EXISTS lenso_files_owner ON lenso_files(realm,owner,state,id);
