# Lenso Modules

Editable functional modules built on the published Lenso SDKs. The first module is
**Auth 0.4.0**: password registration/login and opaque sessions, with independent
TypeScript and Rust implementations, SQLite/PostgreSQL stores and a bounded scrypt provider.

This repository owns business interfaces, implementations, adapters, templates and
behavioral fixtures. SDK primitives live in [lenso](https://github.com/LioRael/lenso),
installation/generation in [lenso-cli](https://github.com/LioRael/lenso-cli), and
curated names/presets in [lenso-registry](https://github.com/LioRael/lenso-registry).
No SDK changes are required. Source consumers use `@lenso/app` / `lenso-app` 0.1.1
from npm/crates.io, not workspace path dependencies.

## Private Files

Files 0.1.0 is published with Auth 0.4.0 in module source v0.4.0. App 0.2.0
provides the shared identity and native streaming host. Both languages support
SQLite/PostgreSQL metadata and local content. See
[Files behavior, variants and acceptance](docs/files.md).

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

For an existing App 0.1.0 project, refresh its app preset, review/apply
`app upgrade-preview` / `app upgrade`, then update Auth with
`code update auth --input '{"reconfigure":true}'`. Existing sessions and user
changes are retained. Explicit App version constraints must be updated to ^0.2.0.
Source/provider changes do not move or delete stored database/blob data.

## Create a native application

App 0.2.0 and Code 0.3.6 add a native host before installing Auth:

```sh
lenso new my-api --preset app
cd my-api
lenso code config --provider lenso.auth-store=sqlite
lenso code preview auth
lenso code add auth
lenso app migrate
lenso app run
```

Choose TypeScript/Node.js 26 or Rust/Tokio (Rust 1.99.0). The empty app exposes
`GET /health`; Auth then adds register/login/session/logout routes. PostgreSQL
uses the same workflow with `lenso.auth-store=postgres` and runtime DATABASE_URL
in the app's `.env`. Preview does not connect to the database. Migrations are
explicit, and application startup checks the existing schema without modifying it.

App records `app_host="lenso-http-v2"` as a fixed project convention. The directory
then chooses `auth.typescript.app-v2.toml` or `auth.rust.app-v2.toml`. These add native
HTTP transport templates over the same SDK-backed business source. The reusable
base catalogs remain available without that convention. Initial App transport
mounts one root instance named `auth`; custom multi-instance hosts use the base
catalogs. SDK and CLI Runtime require no changes. App/Code/module releases and the
official Registry must be synchronized before using these commands publicly.

## Install in a prepared application

Requires Code 0.3.6 and a prepared native application. Install the extension once,
then add functional modules through Code:

```sh
lenso init --preset code
lenso code search auth
lenso code preview auth
lenso code add auth
```

Code owns `lenso.modules.toml` and selects a pinned Auth release. Configure the
project once through commands, then reuse it for previews, adds and updates:

```sh
lenso code config --language typescript --runtime node26
# For Rust: lenso code config --language rust --runtime tokio
lenso code config --modules-path src/features --import-alias '#modules'
lenso code config --provider lenso.auth-store=sqlite
lenso code preview auth
lenso code add auth
```

`lenso.code.toml` records these conventions. JSON/Agent clients can also pass
`code config --input '{"settings":{...}}'` to set module defaults, for example:

```toml
[modules.auth.options]
ttl_ms = 120000
[modules.sqlite.options]
path = "auth.sqlite"
[modules.postgres.options]
connection = { env = "AUTH_DATABASE_URL" }
```

Store connection values stay in the application's environment, not the config or
lock. Project defaults fill missing choices; saved and explicit instance settings
win. With no configured language or store, Code asks through the existing terminal
or JSON questions. `--yes` approves plans and never invents these domain choices.

The default output stays below `src/generated/auth`; `paths.modules` moves its
family root, for example to `src/features/auth`. The generated assembly README
records the selected store, lifetime, factory call and configured TypeScript import
alias. The application owns its TypeScript/runtime alias mapping. Module internals
use relative imports, and Rust needs no TypeScript import alias.

Code stores target context and module versions in `src/code.modules.json` and
separate composition receipts below `src/code/modules`. Repeated adds and updates
reuse configuration. `code preview auth` never writes generated files. Use
`code update auth` to review a newer source release while retaining user edits.

Changing the project provider default does not replace installed storage. Switch
one installed Auth instance explicitly:

```sh
lenso code config --provider lenso.auth-store=postgres
lenso code preview auth --input '{"bindings":{"auth.store":{"type":"module","module":"postgres"}}}'
lenso code add auth --input '{"bindings":{"auth.store":{"type":"module","module":"postgres"}}}'
```

Only the replaced provider's old options are reset; Auth TTL and other instance
settings remain saved. Code applies defaults for the new provider, removes unused
owned sources/dependencies and asks before changing files or running installation
commands. Handwritten entries and database data remain application-owned. A Rust
SQLite-to-PostgreSQL change requires adapting the factory call to async. Data
migration is a separate application operation.

Language/runtime changes require a new project target. Moving an installed output
root requires `code preview auth --reconfigure`, followed by an approved
`code add auth --reconfigure`; handwritten imports are never rewritten for you.
Existing resource catalogs, Auth presets and `code compose` remain compatibility
paths; Auth is a functional Code module rather than a primary extension preset.

The target must already contain `package.json` with ESM/TypeScript build settings,
or `Cargo.toml` with a native Rust entry. Runtime support is Node.js 26 or Rust 1.99
with Tokio. The extension installs editable source and native dependencies; it does
not scaffold a complete application. Explicit database migration remains an
application startup operation. PostgreSQL reads `DATABASE_URL` at runtime; the
composition stores only an environment reference. The generated Rust PostgreSQL
factory is async and requires a Tokio host with I/O enabled.

Provider switches preserve database data and handwritten application entries.
Applications own data conversion and entry changes, including synchronous SQLite
versus async PostgreSQL Rust factory calls. Preview and separate authorization of
files/native commands still apply. Dependency conflicts require a choice.

### Existing Auth installations

The historical `auth` preset and `code compose` commands remain compatibility
paths. Existing configured composition catalogs also accept `code add auth`,
`code preview auth` and `code update auth`, reusing their selected language and
ownership receipt. Applying the Code preset preserves existing extension settings.
To review a newer source ref, explicitly run `lenso ext configure code --input
'{"ref":"v0.2.0"}'` before `code compose-preview --refresh --sample postgres`.
A fresh Code module directory is the primary entry for new projects.

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

`scripts/verify-module-cli.py` is the primary module-directory acceptance. It uses
Code 0.3.6, project language/runtime/path/provider/option defaults, real compilation
and database actions, then switches storage in both directions without clearing
Auth settings. It checks denied plans, retained defaults, source/dependency cleanup,
data preservation, customization-preserving updates and offline replay. Its four
language/store cases run in `Verify Code module installation`; PRs use the local
Auth source with the published Code extension, while manual dispatch consumes the
live official module directory. Use `--local-modules --official` before publishing
Auth; omit `--local-modules` after the immutable release and directory are live.
