CREATE TABLE IF NOT EXISTS lenso_files_migrations(version INTEGER PRIMARY KEY CHECK(version = 1));
INSERT OR IGNORE INTO lenso_files_migrations(version) VALUES(1);
CREATE TABLE IF NOT EXISTS lenso_files (
  realm TEXT NOT NULL, id TEXT NOT NULL, owner TEXT NOT NULL, name TEXT NOT NULL,
  size INTEGER NOT NULL CHECK(size >= 0), sha256 TEXT NOT NULL, created_ms INTEGER NOT NULL,
  state TEXT NOT NULL CHECK(state IN ('pending','ready','deleting')), PRIMARY KEY(realm,id)
);
CREATE INDEX IF NOT EXISTS lenso_files_owner ON lenso_files(realm,owner,state,id);
