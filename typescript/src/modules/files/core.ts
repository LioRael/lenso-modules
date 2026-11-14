import { createHash, randomBytes } from "node:crypto";
import { ActionError, requirePrincipal } from "@lenso/app";
import type { Principal } from "@lenso/app";

export interface FileRecord {
  id: string; owner: string; name: string; size: number; sha256: string; created_ms: number;
}
export interface MetadataStore {
  reserve(file: FileRecord): Promise<void>;
  activate(id: string, owner: string, size: number, sha256: string): Promise<void>;
  find(id: string, owner: string): Promise<FileRecord | undefined>;
  list(owner: string, after: string, limit: number): Promise<FileRecord[]>;
  deleting(id: string, owner: string): Promise<boolean>;
  cancelPending(id:string, owner:string): Promise<boolean>;
  retired(owner: string): Promise<Array<{id:string;state:"pending"|"deleting"}>>;
  finish(id: string, owner: string, state: "pending" | "deleting"): Promise<void>;
}
export interface BlobStore {
  write(id: string, source: AsyncIterable<Uint8Array>): Promise<void>;
  read(id: string): Promise<AsyncIterable<Uint8Array>>;
  remove(id: string): Promise<void>;
}
export function record(row: Record<string, unknown>): FileRecord {
  const {id,owner,name,sha256} = row; const size=Number(row.size), created_ms=Number(row.created_ms);
  if (typeof id!=="string" || typeof owner!=="string" || typeof name!=="string" || typeof sha256!=="string" || !Number.isSafeInteger(size) || size<0 || !Number.isSafeInteger(created_ms)) throw unavailable();
  return {id,owner,name,sha256,size,created_ms};
}
export function validId(id: string): boolean { return /^[a-f0-9]{32}$/.test(id); }
export function validateName(name: string): void {
  if (!name || Buffer.byteLength(name) > 255 || [...name].some(char => char.charCodeAt(0)<32 || char.charCodeAt(0)===127 || char==="/" || char==="\\")) throw new ActionError("invalid_input", "Invalid display filename");
}
export function missing(): ActionError { return new ActionError("not_found", "File not found"); }
export function unavailable(): ActionError { return new ActionError("internal", "File storage unavailable"); }
export class Files {
  constructor(readonly metadata: MetadataStore, readonly blobs: BlobStore, readonly maxBytes = 50 * 1024 * 1024) {
    if (!Number.isSafeInteger(maxBytes) || maxBytes < 1 || maxBytes > 1024 ** 3) throw new ActionError("invalid_input", "Invalid upload limit");
  }
  async upload(principal: Principal | undefined, name: string, source: AsyncIterable<Uint8Array>, signal?: AbortSignal): Promise<FileRecord> {
    const owner = requirePrincipal(principal).subject;
    validateName(name);
    const file: FileRecord = { id: randomBytes(16).toString("hex"), owner, name, size: 0, sha256: "", created_ms: Date.now() };
    await this.metadata.reserve(file);
    const hash = createHash("sha256");
    const max = this.maxBytes;
    async function* checked() {
      for await (const chunk of source) {
        signal?.throwIfAborted();
        file.size += chunk.byteLength;
        if (file.size > max) throw new ActionError("invalid_input", "Upload exceeds configured byte limit");
        hash.update(chunk); yield chunk;
      }
      signal?.throwIfAborted();
    }
    try {
      await this.blobs.write(file.id, checked());
      file.sha256 = hash.digest("hex");
      await this.metadata.activate(file.id, owner, file.size, file.sha256);
      return file;
    } catch (error) {
      // Preserve the pending record if compensation fails so data is never reported as ready.
      try { if (await this.metadata.cancelPending(file.id,owner)) {await this.blobs.remove(file.id); await this.metadata.finish(file.id, owner, "deleting");} }
      catch { throw new ActionError("internal", "Upload cleanup requires recovery", { cause: error }); }
      throw error;
    }
  }
  async stat(principal: Principal | undefined, id: string): Promise<FileRecord> {
    const owner = requirePrincipal(principal).subject;
    if (!validId(id)) throw missing();
    const file=await this.metadata.find(id, owner);
    if (!file) throw missing();
    return file;
  }
  async list(principal: Principal | undefined, after = "", limit = 20): Promise<FileRecord[]> {
    const owner = requirePrincipal(principal).subject;
    if (after && !validId(after) || !Number.isInteger(limit) || limit < 1 || limit > 100) throw new ActionError("invalid_input", "Invalid page");
    return this.metadata.list(owner, after, limit);
  }
  async download(principal: Principal | undefined, id: string) {
    const file = await this.stat(principal, id);
    return { file, source: await this.blobs.read(id) };
  }
  /** Offline maintenance only: stop all writers before recovering interrupted uploads/deletes. */
  async recoverStoppedUploads(principal: Principal | undefined): Promise<number> {
    const owner=requirePrincipal(principal).subject;let recovered=0;
    for (const item of await this.metadata.retired(owner)) {
      await this.blobs.remove(item.id);await this.metadata.finish(item.id,owner,item.state);recovered++;
    }
    return recovered;
  }
  async delete(principal: Principal | undefined, id: string): Promise<void> {
    const owner = requirePrincipal(principal).subject;
    if (!validId(id) || !await this.metadata.deleting(id, owner)) throw missing();
    await this.blobs.remove(id);
    await this.metadata.finish(id, owner, "deleting");
  }
}
