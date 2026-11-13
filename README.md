# Lenso Modules

Editable functional modules built on the published Lenso SDKs. The first module is
**Auth 0.2.0**: password registration/login and opaque sessions, with independent
TypeScript and Rust implementations, SQLite/PostgreSQL stores and a bounded scrypt provider.

This repository owns business interfaces, implementations, adapters, templates and
behavioral fixtures. SDK primitives live in [lenso](https://github.com/LioRael/lenso),
installation/generation in [lenso-cli](https://github.com/LioRael/lenso-cli), and
curated names/presets in [lenso-registry](https://github.com/LioRael/lenso-registry).
No SDK changes are required. Source consumers use `@lenso/app` / `lenso-app` 0.1.1
from npm/crates.io, not workspace path dependencies.

## Install in a prepared application

Requires published CLI 1.0.0 and Code 0.3.1. The official Auth preset asks for the
target language; `--yes` never chooses a language for you. Each application target
has its own root configuration and lock. A monorepo may keep separate TS/Rust
application targets.

```sh
lenso init --preset auth
lenso code validate
lenso code compose-preview --sample default
lenso code compose --sample default
```

The `default` sample explicitly selects SQLite. Select PostgreSQL with
`lenso code compose-preview --sample postgres`, followed by
`lenso code compose --sample postgres`. With no saved composition,
`lenso code compose-preview --input '{"instances":{"auth":"auth"}}'` asks for the
store provider; `--yes` does not choose one. Agents may supply
`--input '{"instances":{"auth":"auth"},"bindings":{"auth.store":{"type":"module","module":"postgres"}}}'`.
The selected provider is recorded in the composition receipt and reused on updates.

PostgreSQL reads `DATABASE_URL` at application runtime; the CLI stores only the
environment reference, never its value. Override the reference with configuration
`{"auth.store":{"connection":{"env":"MY_AUTH_DATABASE_URL"}}}`. Configure TLS in
the URL or native connection configuration for your deployment. The templates do not add a certificate-verification bypass.

Both adapters require explicit migration before use: `await app.store.migrate()`
in TypeScript and `app.store.migrate().await?` in Rust. PostgreSQL migrations use a
transaction and a shared advisory lock across processes and languages, preserve
records and reject a newer schema. PostgreSQL 16 and 18 are tested in CI.

The generated PostgreSQL Rust factory is async: `i_61757468(None).await?`. Its Tokio
host needs I/O enabled. SQLite keeps the existing synchronous factory. TypeScript
factories are synchronous; store operations and PostgreSQL shutdown are async.
SQLite consumers do not acquire PostgreSQL dependencies or source files.

Switching stores reconciles owned code/dependencies after preview and approval. It
does **not** transfer or erase database records. The existing database stays intact;
applications own data conversion and handwritten entry changes. Use the target
sample, or submit the replacement binding and its configuration explicitly.
For an existing Auth 0.1 project, preset reapplication updates package constraints
while retaining installed extension configuration and its source pin. Explicitly
review the new module source ref before previewing:

```sh
lenso preset apply auth
lenso ext configure code --input '{"ref":"v0.2.0"}'
lenso code compose-preview --refresh --sample postgres
lenso code compose --sample postgres
```

The existing language stays selected. Preview does not require a database connection;
set `DATABASE_URL` when running the generated PostgreSQL application.

For Agents, provide `--input '{"language":"typescript"}'` or
`--input '{"language":"rust"}'` during initialization, and use the existing JSON /
answers / approval protocol. Runtime support is Node.js 26 or Tokio on Rust 1.99; selecting
a language installs its native source, SDK dependencies and factory assembly.
Existing project dependency conflicts require a choice and are not bypassed by
`--yes`. Preview and file/command approval precede installation.

The target must already contain `package.json` with ESM/TypeScript build settings,
or `Cargo.toml` with a native Rust entry. The preset installs a module, not a full
application scaffold. Generated source goes below `src/generated/auth` and its
receipt is `src/composition.json`. The default sample exports the factory
`i_61757468`; the `multiple` sample creates independent customer/staff realms.
Handwritten application entries and business extensions remain user-owned.

Use the generated factory with an explicit database path and explicitly run its
SQLite migration before serving traffic. Configure SDK Executor/Context from the
native operation handles. The source host below demonstrates the HTTP integration.
Language/runtime changes require a new application target; code cleanup is not
user-data migration.

## Run the reference hosts from source

Node.js 26 / npm:

```sh
npm ci
npm run build
npm run start --workspace @lenso-modules/auth-source -- --migrate
```

Rust 1.99:

```sh
cargo run --release --manifest-path rust/Cargo.toml --bin auth-server -- --migrate
```

The hosts listen on loopback ports 3000 and 3001. Set `PORT` and `LENSO_AUTH_DB` to
override the defaults; use separate databases unless intentionally exercising the
format-1 interoperability fixture. `--migrate` is explicit and idempotent; without
it, a missing/unsupported schema fails startup. Use release builds for Rust KDF/HTTP
acceptance; unoptimized cryptography is significantly slower.

```sh
curl http://127.0.0.1:3000/auth/register -H 'Content-Type: application/json' \
  -d '{"name":"alice","password":"correct horse battery"}'
curl http://127.0.0.1:3000/auth/login -H 'Content-Type: application/json' \
  -d '{"name":"alice","password":"correct horse battery"}'
curl http://127.0.0.1:3000/auth/session -H 'Authorization: Bearer TOKEN'
curl -X POST http://127.0.0.1:3000/auth/logout -H 'Authorization: Bearer TOKEN'
```

See the [Auth contract and initial scope](contracts/auth/contract.md). Public
registration, password/session semantics and storage are implemented. Production
identity integrations, recovery, MFA, Redis and management UI remain
separate work.

## Authoring and verification

`auth.typescript.toml` and `auth.rust.toml` are complete Code catalogs generated by
`scripts/catalogs.py` from ordinary native source and assembly templates. They live
at the source root so resource paths need no parent traversal. One module family
selects a language first, then resolves business capabilities and adapters.

```sh
python3 scripts/catalogs.py
npm test
npm run lint
cargo fmt --manifest-path rust/Cargo.toml --check
cargo check --locked --manifest-path rust/Cargo.toml --all-targets
cargo test --locked --manifest-path rust/Cargo.toml
cargo clippy --locked --manifest-path rust/Cargo.toml --all-targets -- -D warnings
cargo doc --locked --manifest-path rust/Cargo.toml --no-deps
cargo build --locked --release --manifest-path rust/Cargo.toml --bin auth-server
npm run test:integration
```

`scripts/verify-cli.py` checks both generated consumers using a published CLI,
including language/provider questions, preview/decline/approval, real native dependencies,
compile/run, customization preservation, blocked language changes and offline replay.
Pass `--storage both` with `DATABASE_URL` pointing to a disposable PostgreSQL database
to compile/run all four language/store combinations and switch each to the other
provider, checking source cleanup, dependency cleanup and retained database data.
The harness requires `npm ci` for its private database inspection.
Pass `--official` for the live Registry preset, or `--code-source PATH` for a prebuilt
local Code manifest during extension development. Reports live in the explicit
`--out` directory; SDK and application toolchains remain independent.

Packages in this repository are private build/test consumers. Functional source is
distributed from immutable Git snapshots; it is not an npm/crates.io Auth package.
