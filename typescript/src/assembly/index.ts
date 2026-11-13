import { authModule } from "../modules/auth/core.js";
import { SqliteAuthStore } from "../modules/sqlite/store.js";
import { ScryptHasher } from "../modules/scrypt/hasher.js";
export function createAuth(filename: string, realm = "auth", now = Date.now) {
  const store = new SqliteAuthStore(filename, realm);
  const auth = authModule({ store, passwords: new ScryptHasher(), now, sessionTtlMs: 86_400_000 }, realm);
  return { ...auth, store };
}
