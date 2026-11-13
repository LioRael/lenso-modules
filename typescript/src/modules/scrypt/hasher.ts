import { randomBytes, scrypt, timingSafeEqual } from "node:crypto";
import { ActionError } from "@lenso/app";
import type { PasswordHasher } from "../auth/contracts.js";
const prefix = "scrypt$v1$32768$8$3$";
export class ScryptHasher implements PasswordHasher {
  readonly dummy = prefix + "00".repeat(16) + "$" + "00".repeat(64);
  private active = 0;
  private async derive(password: string, salt: string): Promise<Buffer> {
    if (this.active >= 4) throw new ActionError("internal", "Password worker capacity reached");
    this.active++;
    try {
      return await new Promise<Buffer>((resolve, reject) => {
        scrypt(password, salt, 64, { N: 32768, r: 8, p: 3, maxmem: 64 * 1024 * 1024 }, (error, key) => {
          if (error) reject(error); else resolve(key);
        });
      });
    } finally { this.active--; }
  }
  async hash(password: string): Promise<string> {
    const salt = randomBytes(16).toString("hex");
    return prefix + salt + "$" + (await this.derive(password, salt)).toString("hex");
  }
  async verify(password: string, encoded: string): Promise<boolean> {
    const match = /^scrypt\$v1\$32768\$8\$3\$([0-9a-f]{32})\$([0-9a-f]{128})$/.exec(encoded);
    if (!match?.[1] || !match[2]) throw new ActionError("internal", "Unsupported stored password format");
    return timingSafeEqual(await this.derive(password, match[1]), Buffer.from(match[2], "hex"));
  }
}
