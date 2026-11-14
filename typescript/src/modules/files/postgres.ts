import { Pool } from "pg";
import type { MetadataStore, FileRecord } from "../files/core.js";
import { unavailable, record } from "../files/core.js";
import { schema } from "./postgres-schema.js";
export class PostgresFiles implements MetadataStore {
  private readonly db: Pool;
  constructor(url: string, private readonly realm = "files") {
    this.db = new Pool({connectionString:url,max:4,connectionTimeoutMillis:5000});
    this.db.on("error", () => process.emitWarning("Files PostgreSQL connection failed"));
  }
  async migrate(): Promise<void> {
    const client=await this.db.connect();
    try {
      await client.query("BEGIN"); await client.query("SELECT pg_advisory_xact_lock(1818586739,1718185061)");
      await client.query(schema); const result=await client.query("SELECT MAX(version) AS version FROM lenso_files_migrations");
      if (result.rows[0]?.version!==1) throw unavailable();
      await client.query("COMMIT");
    } catch (error) { await client.query("ROLLBACK"); throw error; }
    finally {client.release();}
  }
  async checkSchema(): Promise<void> { if ((await this.db.query("SELECT MAX(version) AS version FROM lenso_files_migrations")).rows[0]?.version!==1) throw unavailable(); }
  async reserve(file: FileRecord): Promise<void> { await this.db.query("INSERT INTO lenso_files(realm,id,owner,name,size,sha256,created_ms,state) VALUES($1,$2,$3,$4,0,'',$5,'pending')",[this.realm,file.id,file.owner,file.name,file.created_ms]); }
  async activate(id: string, owner: string, size: number, sha256: string): Promise<void> {
    if (!(await this.db.query("UPDATE lenso_files SET state='ready',size=$1,sha256=$2 WHERE realm=$3 AND id=$4 AND owner=$5 AND state='pending'",[size,sha256,this.realm,id,owner])).rowCount) throw unavailable();
  }
  async find(id: string, owner: string): Promise<FileRecord | undefined> {
    const row=(await this.db.query("SELECT id,owner,name,size,sha256,created_ms FROM lenso_files WHERE realm=$1 AND id=$2 AND owner=$3 AND state='ready'",[this.realm,id,owner])).rows[0]; return row ? record(row) : undefined;
  }
  async list(owner: string, after: string, limit: number): Promise<FileRecord[]> { return (await this.db.query("SELECT id,owner,name,size,sha256,created_ms FROM lenso_files WHERE realm=$1 AND owner=$2 AND state='ready' AND id>$3 ORDER BY id LIMIT $4",[this.realm,owner,after,limit])).rows.map(record); }
  async deleting(id: string, owner: string): Promise<boolean> { return Boolean((await this.db.query("UPDATE lenso_files SET state='deleting' WHERE realm=$1 AND id=$2 AND owner=$3 AND state IN ('ready','deleting')",[this.realm,id,owner])).rowCount); }
  async cancelPending(id:string,owner:string): Promise<boolean> {
    return Boolean((await this.db.query("UPDATE lenso_files SET state='deleting' WHERE realm=$1 AND id=$2 AND owner=$3 AND state='pending'",[this.realm,id,owner])).rowCount);
  }
  async retired(owner: string): Promise<Array<{id:string;state:"pending"|"deleting"}>> {
    return (await this.db.query("SELECT id,state FROM lenso_files WHERE realm=$1 AND owner=$2 AND state IN ('pending','deleting') LIMIT 100",[this.realm,owner])).rows.map(row=>{
      if (typeof row.id!=="string" || row.state!=="pending" && row.state!=="deleting") throw unavailable();
      return {id:row.id,state:row.state};
    });
  }
  async finish(id: string, owner: string, state: "pending" | "deleting"): Promise<void> { await this.db.query("DELETE FROM lenso_files WHERE realm=$1 AND id=$2 AND owner=$3 AND state=$4",[this.realm,id,owner,state]); }
  async close(): Promise<void> {await this.db.end();}
}
