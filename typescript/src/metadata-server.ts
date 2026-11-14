/** Acceptance host for the optional SDK descriptions and native streaming Actions. */
import { createServer } from "node:http";
import { randomUUID } from "node:crypto";
import { join } from "node:path";
import { ActionError, Catalog, Executor, requirePrincipal } from "@lenso/app";
import { createAuth } from "./assembly/index.js";
import { Files, filesModule } from "./modules/files/core.js";
import { SqliteFiles } from "./modules/files/sqlite.js";
import { LocalBlobs } from "./modules/files/local.js";
const root = process.env.LENSO_METADATA_ROOT;
if (!root) throw new Error("Missing acceptance directory");
const auth = createAuth(join(root, "auth.sqlite")); auth.store.migrate();
const store = new SqliteFiles(join(root, "files.sqlite")); await store.migrate();
const files = filesModule(new Files(store, await LocalBlobs.open(root), 8));
const catalog = new Catalog(); catalog.register(auth.module); catalog.register(files.module);
const executor = new Executor(() => {});
const server = createServer(async (request, response) => {
  const send = (status: number, value: unknown) => { response.writeHead(status, { "content-type": "application/json", "cache-control": "no-store" }); response.end(JSON.stringify(value)); };
  try {
    const url = new URL(request.url ?? "/", "http://localhost"), token = request.headers.authorization?.replace(/^Bearer /, "");
    const principal = await auth.principal(token);
    const context = { callId: randomUUID(), principal, resources: auth.resources, signal: new AbortController().signal };
    if (url.pathname === "/manifest") { requirePrincipal(principal); send(200, { modules: catalog.describe(), actions: catalog.describeActions() }); return; }
    if (url.pathname === "/auth/register" || url.pathname === "/auth/login") {
      const chunks = []; let size = 0;
      for await (const chunk of request) { size += chunk.length; if (size > 4096) throw new ActionError("invalid_input", "Body too large"); chunks.push(chunk); }
      let input: unknown;
      try { input = JSON.parse(Buffer.concat(chunks).toString()); }
      catch (cause) { throw new ActionError("invalid_input", "Invalid JSON", { cause, issues: [{ path: [], code: "invalid_format" }] }); }
      const result = url.pathname.endsWith("register") ? await executor.callExternal(auth.actions.register, context, input) : await executor.callExternal(auth.actions.login, context, input);
      send(200, result); return;
    }
    const ctx = { ...context, resources: files.resources };
    if (url.pathname === "/files" && request.method === "POST") { send(201, await executor.call(files.actions.upload, ctx, { name: url.searchParams.get("name") ?? "", source: request })); return; }
    if (url.pathname === "/files" && request.method === "GET") { send(200, { files: await executor.call(files.actions.list, ctx, { after: url.searchParams.get("after") ?? "", limit: Number(url.searchParams.get("limit") ?? 20) }) }); return; }
    const id = url.pathname.split("/")[2] ?? "";
    if (url.pathname === `/files/${id}/content` && request.method === "GET") {
      const { file, source } = await executor.call(files.actions.download, ctx, { id });
      response.writeHead(200, { "content-length": file.size, "content-type": "application/octet-stream" });
      for await (const chunk of source) { if (!response.write(chunk)) await new Promise<void>(resolve => response.once("drain", resolve)); } response.end(); return;
    }
    if (url.pathname === `/files/${id}` && request.method === "DELETE") { await executor.call(files.actions.delete, ctx, { id }); send(200, { deleted: true }); return; }
    if (url.pathname === `/files/${id}` && request.method === "GET") { send(200, await executor.call(files.actions.stat, ctx, { id })); return; }
    send(404, { code: "not_found" });
  } catch (cause) {
    const error = cause instanceof ActionError ? cause : new ActionError("internal", "Operation failed", { cause });
    if (response.headersSent) { response.destroy(); return; }
    send(({ invalid_input: 400, unauthenticated: 401, forbidden: 403, not_found: 404, conflict: 409, internal: 500, cancelled: 499 })[error.code], error.toJSON());
  }
});
server.listen(0, "127.0.0.1", () => { const address = server.address(); if (address && typeof address !== "string") console.log(`LISTENING ${address.port}`); });
process.once("SIGTERM", () => server.close(() => { auth.store.close(); store.close(); process.exit(0); }));
