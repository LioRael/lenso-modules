import { DatabaseSync } from "node:sqlite";
import type { FileRecord, MetadataStore } from "../files/core.js";
import { unavailable, record } from "../files/core.js";
import { schema } from "./sqlite-schema.js";
export class SqliteFiles implements MetadataStore {
  private readonly db: DatabaseSync;
  constructor(path: string, private readonly realm = "files") {
    this.db = new DatabaseSync(path); this.db.exec("PRAGMA busy_timeout=5000; PRAGMA journal_mode=WAL");
  }
  async migrate(): Promise<void> {
    this.db.exec("BEGIN IMMEDIATE");
    try { this.db.exec(schema); await this.checkSchema(); this.db.exec("COMMIT"); }
    catch (error) { this.db.exec("ROLLBACK"); throw error; }
  }
  async checkSchema(): Promise<void> {
    if (this.db.prepare("SELECT MAX(version) AS version FROM lenso_files_migrations").get()?.version !== 1) throw unavailable();
  }
  async reserve(file: FileRecord): Promise<void> {
    this.db.prepare("INSERT INTO lenso_files(realm,id,owner,name,size,sha256,created_ms,state) VALUES(?,?,?,?,?,?,?,'pending')").run(this.realm,file.id,file.owner,file.name,0,"",file.created_ms);
  }
  async activate(id: string, owner: string, size: number, sha256: string): Promise<void> {
    if (!this.db.prepare("UPDATE lenso_files SET state='ready',size=?,sha256=? WHERE realm=? AND id=? AND owner=? AND state='pending'").run(size,sha256,this.realm,id,owner).changes) throw unavailable();
  }
  async find(id: string, owner: string): Promise<FileRecord | undefined> {
    const row=this.db.prepare("SELECT id,owner,name,size,sha256,created_ms FROM lenso_files WHERE realm=? AND id=? AND owner=? AND state='ready'").get(this.realm,id,owner); return row ? record(row) : undefined;
  }
  async list(owner: string, after: string, limit: number): Promise<FileRecord[]> {
    return this.db.prepare("SELECT id,owner,name,size,sha256,created_ms FROM lenso_files WHERE realm=? AND owner=? AND state='ready' AND id>? ORDER BY id LIMIT ?").all(this.realm,owner,after,limit).map(record);
  }
  async deleting(id: string, owner: string): Promise<boolean> {
    return Boolean(this.db.prepare("UPDATE lenso_files SET state='deleting' WHERE realm=? AND id=? AND owner=? AND state IN ('ready','deleting')").run(this.realm,id,owner).changes);
  }
  async cancelPending(id:string,owner:string): Promise<boolean> {
    return Boolean(this.db.prepare("UPDATE lenso_files SET state='deleting' WHERE realm=? AND id=? AND owner=? AND state='pending'").run(this.realm,id,owner).changes);
  }
  async retired(owner: string): Promise<Array<{id:string;state:"pending"|"deleting"}>> {
    return this.db.prepare("SELECT id,state FROM lenso_files WHERE realm=? AND owner=? AND state IN ('pending','deleting') LIMIT 100").all(this.realm,owner).map(row=>{
      if (typeof row.id!=="string" || row.state!=="pending" && row.state!=="deleting") throw unavailable();
      return {id:row.id,state:row.state};
    });
  }
  async finish(id: string, owner: string, state: "pending" | "deleting"): Promise<void> {
    this.db.prepare("DELETE FROM lenso_files WHERE realm=? AND id=? AND owner=? AND state=?").run(this.realm,id,owner,state);
  }
  async close(): Promise<void> { this.db.close(); }
}
