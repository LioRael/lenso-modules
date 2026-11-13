# Auth 0.1 contract

Auth is a native SDK module family. Its initial implementations are TypeScript on
Node.js 26 and Rust 1.99 with Tokio. Both consume published App SDK 0.1.1; neither depends on a
CLI process at application runtime. SQLite and scrypt are the first adapters.

## Operations

- Register accepts an ASCII username matching `[A-Za-z0-9_-]{3,64}` and a password
  of 12–128 UTF-8 bytes. Usernames normalize to ASCII lowercase; uniqueness is
  atomic within a configured realm. Duplicate names return `conflict`.
- Login verifies the encoded password, creates an independent opaque token and
  returns `{token, expiresMs}`. Missing users and incorrect passwords return the
  same `unauthenticated` error. Unknown users still perform the configured KDF work.
- Session lookup accepts a bearer token at the trusted host boundary and resolves
  its realm-scoped stored subject only while `expiresMs > now`. No caller-supplied
  subject/scopes are trusted. A resolved SDK principal initially has no scopes;
  applications own downstream authorization policy.
- Current-session Actions require a trusted principal and return public user data.
  They do not re-establish identity from arbitrary native caller input: hosts must
  construct a fresh context after token verification.
- Logout requires a trusted principal and a matching, unexpired token. It revokes
  that token only. Revocation does not affect other tokens or another realm.

Public user output contains only id/name. Raw tokens are returned only on login;
the store persists their SHA-256 digests. Stored password hashes and salts are never
returned through Actions. The lifetime defaults to 24 hours and accepts 60 seconds
to 30 days. Expiry is absolute, not sliding. Clock injection supports deterministic
behavioral tests; external requests cannot choose the application clock.

## Interfaces and persistence

The module depends on native AuthStore and PasswordHasher interfaces. Store methods
must preserve realm isolation, uniqueness and atomic session writes. Adapters map
expected conflicts into SDK errors and retain unexpected failures privately. The
same behavioral fixtures apply to each language/provider.

The SQLite adapter opens a native connection and requires an explicit `migrate()`
operation. Schema 1 uses module-prefixed tables and a dedicated migration ledger;
it does not overwrite application `PRAGMA user_version`. A newer recorded schema
is rejected. Migration is transactional and idempotent; no database files or
application records are deleted by module removal or Code generation.

Scrypt format 1 uses `N=32768,r=8,p=3`, 16 random salt bytes represented as hex text,
and a 64-byte result. Both implementations use the UTF-8 bytes of that salt text.
The versioned encoding pins these parameters; malformed/unknown parameter profiles
are rejected before allocation. This profile is one of the
[OWASP scrypt configurations](https://github.com/OWASP/CheatSheetSeries/blob/master/cheatsheets/Password_Storage_Cheat_Sheet.md#scrypt).
Use the same password-adapter interface for future reviewed providers; changing
providers must account for already stored credentials.

KDF execution is capped at four active operations per hasher. Rust keeps its permit
inside the blocking job even if its awaiting future is dropped. Application-wide
admission and abuse/rate-limit policy remain host responsibilities.

## HTTP reference host and scope

The explicit host maps POST `/auth/register`, POST `/auth/login`, GET
`/auth/session` and POST `/auth/logout` to SDK Actions. It uses Authorization bearer
tokens, JSON, 4 KiB request limits, no-store responses, a four-request admission
limit and loopback-only binding. It has no implicit cookie authentication or CORS
policy. Production TLS, account/IP rate limits and deployment hosting belong to
the integrating application.

This is an initial password/session module, not a complete production identity
provider. Email verification, recovery, MFA, OAuth/OIDC, disabled-account policy,
roles, Redis, PostgreSQL and automated database/data conversion are not implemented.
SDK cancellation and an interrupted response do not imply rollback of a committed
registration or session operation.

Tests cover shared input fixtures, public failures, realm isolation, expiry,
revocation, SQLite uniqueness, schema rejection, restart persistence, real HTTP and
TS/Rust format-1 database/session interoperability. Format-1 interoperability is
version-specific evidence, not an unversioned permanent schema guarantee.
