import { randomBytes } from "node:crypto";
import { Pool } from "pg";
import type { PoolConfig, QueryResultRow } from "pg";
import { ActionError } from "@lenso/app";
import { z } from "zod";
import { userSchema, validInstanceId } from "../auth/contracts.js";
import type { AuthStore, User } from "../auth/contracts.js";
import { schema } from "./schema.js";

function unavailable(): ActionError { return new ActionError("internal", "Authentication PostgreSQL store unavailable"); }
export class PostgresAuthStore implements AuthStore {
  private readonly pool: Pool;
  private closing: Promise<void> | undefined;
  constructor(config: PoolConfig, readonly realm = "auth") {
    if (!validInstanceId(realm)) throw new ActionError("invalid_input", "Invalid PostgreSQL realm");
    this.pool = new Pool({ max: 4, connectionTimeoutMillis: 5000, ...config });
    this.pool.on("error", () => process.emitWarning("Authentication PostgreSQL idle connection failed"));
  }
  private async rows(sql: string, values: readonly (string | number)[] = []): Promise<QueryResultRow[]> {
    try { return (await this.pool.query(sql, [...values])).rows; }
    catch { throw unavailable(); }
  }
  async migrate(): Promise<void> {
    const client = await this.pool.connect().catch(() => { throw unavailable(); });
    let discard = false;
    try {
      await client.query("BEGIN");
      // Serialize migrations across processes and languages, including first creation.
      await client.query("SELECT pg_advisory_xact_lock(1818586739, 1635087464)");
      await client.query(schema);
      const result = await client.query("SELECT MAX(version) AS version FROM lenso_auth_migrations");
      if (result.rows[0]?.version !== 1) throw new ActionError("internal", "Unsupported authentication schema version");
      await client.query("COMMIT");
    } catch (error) {
      try { await client.query("ROLLBACK"); } catch { discard = true; }
      throw error instanceof ActionError ? error : unavailable();
    } finally { client.release(discard); }
  }
  async checkSchema(): Promise<void> {
    const rows = await this.rows("SELECT MAX(version) AS version FROM lenso_auth_migrations");
    if (rows[0]?.version !== 1) throw new ActionError("internal", "Run the authentication migration first");
  }
  async createUser(name: string, passwordHash: string): Promise<User> {
    const id = randomBytes(16).toString("hex");
    const rows = await this.rows("INSERT INTO lenso_auth_users(realm,id,name,password_hash) VALUES($1,$2,$3,$4) ON CONFLICT(realm,name) DO NOTHING RETURNING id,name", [this.realm,id,name,passwordHash]);
    if (!rows.length) throw new ActionError("conflict", "User already exists");
    return userSchema.parse(rows[0]);
  }
  async credential(name: string) {
    const rows = await this.rows("SELECT id,name,password_hash AS \"passwordHash\" FROM lenso_auth_users WHERE realm=$1 AND name=$2", [this.realm,name]);
    if (!rows[0]) return undefined;
    const value = userSchema.extend({ passwordHash: z.string() }).parse(rows[0]);
    return { user: { id: value.id, name: value.name }, passwordHash: value.passwordHash };
  }
  async user(id: string): Promise<User | undefined> {
    const rows = await this.rows("SELECT id,name FROM lenso_auth_users WHERE realm=$1 AND id=$2", [this.realm,id]);
    return rows[0] ? userSchema.parse(rows[0]) : undefined;
  }
  async saveSession(hash: string, subject: string, expiresMs: number): Promise<void> {
    await this.rows("INSERT INTO lenso_auth_sessions(realm,token_hash,subject,expires_ms) VALUES($1,$2,$3,$4)", [this.realm,hash,subject,expiresMs]);
  }
  async session(hash: string, nowMs: number): Promise<User | undefined> {
    const rows = await this.rows("SELECT u.id,u.name FROM lenso_auth_sessions s JOIN lenso_auth_users u ON u.realm=s.realm AND u.id=s.subject WHERE s.realm=$1 AND s.token_hash=$2 AND s.expires_ms>$3", [this.realm,hash,nowMs]);
    return rows[0] ? userSchema.parse(rows[0]) : undefined;
  }
  async revokeSession(hash: string, subject: string): Promise<void> {
    await this.rows("DELETE FROM lenso_auth_sessions WHERE realm=$1 AND token_hash=$2 AND subject=$3", [this.realm,hash,subject]);
  }
  close(): Promise<void> { return this.closing ??= this.pool.end(); }
}
