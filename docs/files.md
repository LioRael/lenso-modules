# Private Files v1

Status: published Files 0.1.0 and Auth 0.4.0 from module source v0.4.0, with
App 0.2.0 host protocol v2 and Code 0.3.6. The official app preset and Registry
select the compatible extension versions. Files remains a Code module.
The SDK and CLI Runtime are unchanged.

## Ownership and variants

Files owns metadata, binary content lifecycle, owner-scoped operations, migrations
and native HTTP adapters. TypeScript/Node.js 26 and Rust/Tokio are independent
implementations over published `@lenso/app` / `lenso-app` 0.1.1. The core uses SDK
trusted-principal checks; client JSON/query fields never establish identity.

Two independently selected ports separate metadata from content:

- `lenso.files-metadata` v1: SQLite or PostgreSQL; owner-scoped reservation,
  activation, listing, lookup and retirement.
- `lenso.blob-store` v1: local filesystem; generated object keys, bounded streams,
  durable completion and idempotent physical removal.

Each language has one business implementation. Catalogs choose providers and
render assembly; they do not fork business behavior for each combination. Auth's
store and Files metadata are separate native interfaces. They may explicitly use
one database, but have independent table prefixes and migrations. There is no SQL
foreign key or direct import into Auth internals. The Files HTTP variant initially
uses the primary root instance `files`, alongside one application identity provider.

## Project flow

```sh
lenso new my-api --preset app
cd my-api
lenso code config --provider lenso.auth-store=sqlite
lenso code config --provider lenso.files-metadata=sqlite
lenso code add auth
lenso code preview files
lenso code add files
lenso app check
lenso app migrate
lenso app run
```

The project language/runtime, module path and host convention are reused from
`lenso.code.toml`. Existing configuration and bindings remain authoritative.
Files options include `max_bytes` (1..1 GiB, default 50 MiB). SQLite defaults to
`files.sqlite`; local content defaults to `data/files/files/`. PostgreSQL reads
an environment reference (default DATABASE_URL) from the native app's `.env`.
Credentials are never placed in Code configuration or source locks. App ignores
its conventional `data/` directory. Custom content directories need appropriate
application-owned Git ignore and filesystem permissions.

## HTTP behavior

- `POST /files?name=...`: authenticated raw binary upload; returns 201 and metadata.
- `GET /files?after=ID&limit=N`: this owner's ready files, ordered by ID; limit 1..100.
- `GET /files/{id}`: this owner's ready file metadata.
- `GET /files/{id}/content`: authenticated binary download with attachment,
  no-store and nosniff response headers.
- `DELETE /files/{id}`: retire and delete this owner's file.

A missing/revoked/expired identity returns 401. Cross-owner access returns the same
404 as a missing object. IDs are server-generated 128-bit random keys. Display
names are UTF-8 up to 255 bytes with path separators/control bytes rejected; they
never become object paths. Content is opaque and never served inline. There is no
public URL, trusted MIME claim, image transformation or archive extraction.

Uploads count actual bytes and hash incrementally. Local content is addressed
under a private root, with exclusive creation and symlink-safe reads (directory
capabilities in Rust). HTTP request/upload deadlines and concurrency limits bound
native work. Stream cancellation closes readers and compensates pending uploads.
Each new operation resolves the session again; no session/permission cache is added.

## Failure and maintenance semantics

Metadata moves from pending to ready after content finishes, or to deleting before
physical deletion. Pending/deleting entries are excluded from normal lookup and
listing. Metadata and content are separate stores; no cross-store atomic transaction
or automatic data rollback is promised.

Failed writes and oversize/cancelled uploads attempt compensation. Cleanup first
claims the pending state, so an uncertain database acknowledgement cannot delete
content already committed as ready. A failed delete stays deleting and can be
retried using the original ID. Rust task cancellation schedules pending cleanup.
Abrupt process termination can still leave interrupted records.

`Files.recoverStoppedUploads(principal)` (TypeScript) and
`Files::recover_stopped_uploads(principal)` (Rust) are explicit native maintenance
APIs. Stop **all** writers first. Each call handles at most 100 pending/deleting
records belonging to that trusted subject; repeat until zero. They are deliberately
not exposed as an ordinary HTTP route and are not run by schema migrations. The
operator must establish the trusted maintenance principal in native application
code. These methods must never be called concurrently with active uploads.

Code preview/add/update touches generated source and reviewed native manifests,
not stored content or database rows. Updating providers does not move existing
data. Database/object migration needs a separate operator-reviewed transfer and
verification procedure. Removing generated source must not delete data directories.

## Acceptance

`lenso-cli/scripts/verify-files.py` uses actual packaged Wasm and Code workflows,
creates native projects, and checks all four language/metadata combinations against
real local files and SQLite/PostgreSQL. It verifies >1 MiB chunked uploads, byte/hash
round trips, two-user isolation, forged owner rejection, revocation, oversize and
interrupted upload cleanup, restart persistence and physical deletion. CLI checks
cover preview, denied changes, owned updates, customization and locked/offline replay.
`--legacy-app` also verifies App 0.1.0 upgrade and existing Auth session preservation.
Native unit tests inject failed commits/removals, lost acknowledgements, task
cancellation and outside-root symlinks. No S3, sharing, organization roles, resumable
upload, scanning, Console UI or automatic data migration is advertised.
