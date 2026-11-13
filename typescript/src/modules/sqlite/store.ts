import { randomBytes } from "node:crypto";
import { DatabaseSync } from "node:sqlite";
import { ActionError } from "@lenso/app";
import { z } from "zod";
import { userSchema, validInstanceId } from "../auth/contracts.js";
import type { AuthStore, User } from "../auth/contracts.js";
import { schema } from "./schema.js";
export class SqliteAuthStore implements AuthStore {
  private readonly db: DatabaseSync;
  constructor(filename: string, readonly realm = "auth") {
    if (!filename || !validInstanceId(realm)) throw new ActionError("invalid_input", "Invalid SQLite configuration");
    this.db = new DatabaseSync(filename, { timeout: 5000 });
    try { this.db.exec("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;"); }
    catch (error) { this.db.close(); throw error; }
  }
  migrate(): void {
    this.db.exec("BEGIN IMMEDIATE");
    try {
      this.db.exec(schema);
      const row = this.db.prepare("SELECT MAX(version) AS version FROM lenso_auth_migrations").get();
      if (row?.version !== 1) throw new ActionError("internal", "Unsupported authentication schema version");
      this.db.exec("COMMIT");
    } catch (error) { this.db.exec("ROLLBACK"); throw error; }
  }
  checkSchema(): void {
    const row = this.db.prepare("SELECT MAX(version) AS version FROM lenso_auth_migrations").get();
    if (row?.version !== 1) throw new ActionError("internal", "Run the authentication migration first");
  }
  async createUser(name: string, passwordHash: string): Promise<User> {
    const id = randomBytes(16).toString("hex");
    const result = this.db.prepare("INSERT INTO lenso_auth_users(realm,id,name,password_hash) VALUES(?,?,?,?) ON CONFLICT(realm,name) DO NOTHING").run(this.realm,id,name,passwordHash);
    if (result.changes === 0) throw new ActionError("conflict", "User already exists");
    return { id, name };
  }
  async credential(name: string) {
    const row = this.db.prepare("SELECT id,name,password_hash AS passwordHash FROM lenso_auth_users WHERE realm=? AND name=?").get(this.realm,name);
    if (!row) return undefined;
    const value = userSchema.extend({ passwordHash: z.string() }).parse(row);
    return { user: { id: value.id, name: value.name }, passwordHash: value.passwordHash };
  }
  async user(id: string): Promise<User | undefined> {
    const row = this.db.prepare("SELECT id,name FROM lenso_auth_users WHERE realm=? AND id=?").get(this.realm,id);
    return row ? userSchema.parse(row) : undefined;
  }
  async saveSession(hash: string, subject: string, expiresMs: number): Promise<void> {
    this.db.prepare("INSERT INTO lenso_auth_sessions(realm,token_hash,subject,expires_ms) VALUES(?,?,?,?)").run(this.realm,hash,subject,expiresMs);
  }
  async session(hash: string, nowMs: number): Promise<User | undefined> {
    const row = this.db.prepare("SELECT u.id,u.name FROM lenso_auth_sessions s JOIN lenso_auth_users u ON u.realm=s.realm AND u.id=s.subject WHERE s.realm=? AND s.token_hash=? AND s.expires_ms>?").get(this.realm,hash,nowMs);
    return row ? userSchema.parse(row) : undefined;
  }
  async revokeSession(hash: string, subject: string): Promise<void> {
    this.db.prepare("DELETE FROM lenso_auth_sessions WHERE realm=? AND token_hash=? AND subject=?").run(this.realm,hash,subject);
  }
  close(): void { this.db.close(); }
}
