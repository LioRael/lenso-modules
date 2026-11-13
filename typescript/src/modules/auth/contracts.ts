import { z } from "zod";
export const userSchema = z.object({ id: z.string(), name: z.string() });
export type User = z.infer<typeof userSchema>;
export interface Credential { readonly user: User; readonly passwordHash: string }
export interface AuthStore {
  createUser(name: string, passwordHash: string): Promise<User>;
  credential(name: string): Promise<Credential | undefined>;
  user(id: string): Promise<User | undefined>;
  saveSession(tokenHash: string, subject: string, expiresMs: number): Promise<void>;
  session(tokenHash: string, nowMs: number): Promise<User | undefined>;
  revokeSession(tokenHash: string, subject: string): Promise<void>;
}
export interface PasswordHasher {
  hash(password: string): Promise<string>;
  verify(password: string, encoded: string): Promise<boolean>;
  readonly dummy: string;
}
export interface AuthResources {
  readonly store: AuthStore;
  readonly passwords: PasswordHasher;
  readonly now: () => number;
  readonly sessionTtlMs: number;
}

export function validInstanceId(id: string): boolean { return /^[A-Za-z0-9_-]{1,64}(\.[A-Za-z0-9_-]{1,64}){0,31}$/.test(id); }
