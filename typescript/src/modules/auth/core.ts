import { createHash, randomBytes } from "node:crypto";
import { ActionError, Module, defineAction, emptyInput, requirePrincipal } from "@lenso/app";
import type { Principal } from "@lenso/app";
import { z } from "zod";
import { userSchema, validInstanceId } from "./contracts.js";
import type { AuthResources } from "./contracts.js";

export const credentialsInput = z.object({
  name: z.string().regex(/^[A-Za-z0-9_-]{3,64}$/),
  password: z.string().refine(value => Buffer.byteLength(value) >= 12 && Buffer.byteLength(value) <= 128),
}).strict();
export const tokenInput = z.object({ token: z.string().regex(/^[0-9a-f]{64}$/) }).strict();
export function tokenHash(token: string): string { return createHash("sha256").update(token).digest("hex"); }
function time(resources: AuthResources): number {
  const now = resources.now();
  if (!Number.isSafeInteger(now) || now < 0 || !Number.isSafeInteger(now + resources.sessionTtlMs)) {
    throw new ActionError("internal", "Invalid authentication clock");
  }
  return now;
}
export function authModule(resources: AuthResources, id = "auth") {
  if (!validInstanceId(id) || !Number.isSafeInteger(resources.sessionTtlMs)
      || resources.sessionTtlMs < 60_000 || resources.sessionTtlMs > 2_592_000_000) {
    throw new ActionError("invalid_input", "Invalid authentication configuration");
  }
  const action = defineAction<AuthResources>();
  const register = action({
    id: `${id}.register`, input: credentialsInput, output: userSchema,
    authorize() {},
    async run(context, input) {
      return context.resources.store.createUser(input.name.toLowerCase(), await context.resources.passwords.hash(input.password));
    },
  });
  const login = action({
    id: `${id}.login`, input: credentialsInput, output: z.object({ token: z.string(), expiresMs: z.number().int() }),
    authorize() {},
    async run(context, input) {
      const r = context.resources;
      const credential = await r.store.credential(input.name.toLowerCase());
      const valid = await r.passwords.verify(input.password, credential?.passwordHash ?? r.passwords.dummy);
      if (!valid || !credential) throw new ActionError("unauthenticated", "Invalid credentials");
      const token = randomBytes(32).toString("hex");
      const expiresMs = time(r) + r.sessionTtlMs;
      await r.store.saveSession(tokenHash(token), credential.user.id, expiresMs);
      return { token, expiresMs };
    },
  });
  const session = action({
    id: `${id}.session`, input: emptyInput, output: userSchema,
    authorize(context) { requirePrincipal(context.principal); },
    async run(context) {
      const user = await context.resources.store.user(requirePrincipal(context.principal).subject);
      if (!user) throw new ActionError("unauthenticated", "Authentication required");
      return user;
    },
  });
  const logout = action({
    id: `${id}.logout`, input: tokenInput, output: z.object({ revoked: z.literal(true) }),
    async authorize(context, input) {
      const principal = requirePrincipal(context.principal);
      const user = await context.resources.store.session(tokenHash(input.token), time(context.resources));
      if (!user || user.id !== principal.subject) throw new ActionError("unauthenticated", "Authentication required");
    },
    async run(context, input) {
      await context.resources.store.revokeSession(tokenHash(input.token), requirePrincipal(context.principal).subject);
      return { revoked: true as const };
    },
  });
  const actions = { register, login, session, logout };
  const module = new Module(id, { actions, resources: [`${id}.store`, `${id}.passwords`] });
  async function principal(token: string | undefined): Promise<Principal | undefined> {
    if (!token || !/^[0-9a-f]{64}$/.test(token)) return undefined;
    const user = await resources.store.session(tokenHash(token), time(resources));
    return user ? { subject: user.id, scopes: [] } : undefined;
  }
  return { module, actions, resources, principal };
}
