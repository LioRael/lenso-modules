import { constants } from "node:fs";
import { mkdir, open, realpath, lstat, unlink } from "node:fs/promises";
import { join, resolve } from "node:path";
import type { BlobStore } from "../files/core.js";
import { validId, unavailable } from "../files/core.js";

export class LocalBlobs implements BlobStore {
  private constructor(private readonly root: string) {}
  static async open(directory: string, realm = "files"): Promise<LocalBlobs> {
    if (!/^[a-zA-Z0-9_-]{1,64}$/.test(realm)) throw unavailable();
    const root = resolve(directory, realm);
    await mkdir(root, { recursive: true, mode: 0o700 });
    if ((await lstat(root)).isSymbolicLink()) throw unavailable();
    return new LocalBlobs(await realpath(root));
  }
  private async path(id: string): Promise<string> {
    if (!validId(id) || await realpath(this.root) !== this.root) throw unavailable();
    return join(this.root, id);
  }
  async write(id: string, source: AsyncIterable<Uint8Array>): Promise<void> {
    const file = await open(await this.path(id), constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | constants.O_NOFOLLOW, 0o600);
    try {
      for await (const chunk of source) {
        let written = 0;
        while (written < chunk.length) written += (await file.write(chunk, written, chunk.length - written)).bytesWritten;
      }
      await file.sync();
    } finally { await file.close(); }
  }
  async read(id: string): Promise<AsyncIterable<Uint8Array>> {
    const file = await open(await this.path(id), constants.O_RDONLY | constants.O_NOFOLLOW);
    if (!(await file.stat()).isFile()) { await file.close(); throw unavailable(); }
    return file.createReadStream({ autoClose: true });
  }
  async remove(id: string): Promise<void> {
    try { await unlink(await this.path(id)); }
    catch (error) { if (!(error instanceof Error && "code" in error && error.code === "ENOENT")) throw error; }
  }
}
