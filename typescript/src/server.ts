import { createServer } from "node:http";
import { randomBytes } from "node:crypto";
import { Executor, ActionError } from "@lenso/app";
import { createAuth } from "./assembly/index.js";
const app = createAuth(process.env.LENSO_AUTH_DB ?? "auth.sqlite");
if (process.argv.includes("--migrate")) app.store.migrate();
app.store.checkSchema();
const executor = new Executor(() => {});
let active = 0;
const server = createServer(async (request, response) => {
  const send = (status: number, value: unknown) => {
    if (!response.destroyed) { response.writeHead(status, { "Content-Type": "application/json", "Cache-Control": "no-store" }); response.end(JSON.stringify(value)); }
  };
  if (active >= 4) { request.resume(); send(429, { code: "capacity", message: "Try again later" }); return; }
  active++;
  try {
    const route = `${request.method} ${request.url}`;
    if (!["POST /auth/register", "POST /auth/login", "POST /auth/logout", "GET /auth/session"].includes(route)) {
      request.resume(); send(404, { code: "not_found" }); return;
    }
    const header = request.headers.authorization;
    const token = header?.startsWith("Bearer ") ? header.slice(7) : undefined;
    const context = { callId: randomBytes(16).toString("hex"), resources: app.resources,
      principal: await app.principal(token), signal: new AbortController().signal };
    let input: unknown = null;
    if (route === "POST /auth/register" || route === "POST /auth/login") {
      if (request.headers["content-type"]?.split(";")[0] !== "application/json") { request.resume(); send(415, { code: "invalid_input" }); return; }
      let size = 0;
      const chunks: Buffer[] = [];
      for await (const value of request.iterator({ destroyOnReturn: false })) {
        const chunk = Buffer.from(value); size += chunk.length;
        if (size > 4096) { request.resume(); send(413, { code: "invalid_input" }); return; }
        chunks.push(chunk);
      }
      try { input = JSON.parse(Buffer.concat(chunks).toString("utf8")); }
      catch { send(400, { code: "invalid_input" }); return; }
    } else { request.resume(); }
    let result: unknown;
    switch (route) {
      case "POST /auth/register": result = await executor.callExternal(app.actions.register, context, input); break;
      case "POST /auth/login": result = await executor.callExternal(app.actions.login, context, input); break;
      case "POST /auth/logout": result = await executor.callExternal(app.actions.logout, context, { token: token ?? "" }); break;
      default: result = await executor.callExternal(app.actions.session, context, null);
    }
    send(200, result);
  } catch (cause) {
    const error = cause instanceof ActionError ? cause : new ActionError("internal", "Authentication operation failed", { cause });
    const status = { invalid_input: 400, unauthenticated: 401, forbidden: 403, conflict: 409, not_found: 404, cancelled: 499, internal: 500 }[error.code];
    send(status, { code: error.code, message: error.code === "internal" ? "Authentication operation failed" : error.message });
  } finally { active--; }
});
server.requestTimeout = 10_000;
server.headersTimeout = 10_000;
server.listen(Number(process.env.PORT ?? "3000"), "127.0.0.1", () => {
  const address = server.address();
  if (address && typeof address !== "string") console.log(`LISTENING ${address.port}`);
});
for (const signal of ["SIGINT", "SIGTERM"] as const) process.once(signal, () => server.close(() => { app.store.close(); process.exit(0); }));
