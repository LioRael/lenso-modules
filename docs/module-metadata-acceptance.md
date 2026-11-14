# Auth/Files SDK metadata acceptance

## SDK publication and registry consumers

The compatible SDK is published: `@lenso/app` 0.1.2, `lenso-app` 0.1.2 and `@lenso/console` 3.1.0. [SDK publication 43639576110](https://github.com/LioRael/lenso/actions/runs/43639576110) completed through OIDC trusted publishing; npm provenance and crates.io trusted publisher data identify source commit `13a83df807cbd6ca520954dc2c1b35c2b7d52107`. SQLite SDK packages retain 0.1.1. Module manifests, npm/Cargo locks and generated Auth/Files catalogs now use registry App 0.1.2. The temporary SDK override has been removed.

A clean temporary registry consumer on macOS arm64 passed the same 10 TS, 10 Rust and five real-process HTTP cases, together with lint, native Cargo check/Clippy/docs/format. Its [verification record](module-metadata-acceptance/registry-macos/verification.json), [TS report](module-metadata-acceptance/registry-macos/typescript.json), [Rust report](module-metadata-acceptance/registry-macos/rust.json) and [HTTP log](module-metadata-acceptance/registry-macos/integration.log) preserve actual results. This run used Node.js 26.10.0 and Rust 1.99.0; PostgreSQL acceptance remains separately recorded below and in hosted CI.

## Hosted cross-platform acceptance

Implementation head `f6e35616f92d298f7e6542d21fbc24d3b6e51f3d` passed all eight [source CI jobs](https://github.com/LioRael/lenso-modules/actions/runs/43640925161) and all four [full CLI jobs](https://github.com/LioRael/lenso-modules/actions/runs/43640925090). The [cross-platform record](module-metadata-acceptance/cross-platform.json) preserves run/head/artifact identities, verified ZIP digests, platform facts, actual installed SDK versions and per-project behavior results. [SDK registry facts](module-metadata-acceptance/sdk-release.json) retain package integrity and publishing provenance.

| Check | Ubuntu 24.04 x86_64 | macOS 15 arm64 |
| --- | --- | --- |
| TS strict build, lint and behavioral regression | 10 passed | 10 passed |
| Rust check, tests, strict Clippy, docs and fmt | 10 passed; PostgreSQL case separately exercised | 10 passed; PostgreSQL case separately exercised |
| Real-process Auth/Files HTTP regression | Five passed | Five passed |
| Full TypeScript CLI projects | Eight passed | Eight passed |
| Full Rust CLI projects | Eight passed | Eight passed |

Hosted runs use Node.js 26.13.1 and Rust 1.99.0. Native Auth PostgreSQL tests additionally pass on Linux against PostgreSQL 16 and 18, one TS and one Rust test per server version. The full CLI matrix uses disposable PostgreSQL 18 on both platforms and SQLite. All disposable database services are explicitly stopped.

Each language/platform validates eight final projects: two Auth consumers with source updates and bidirectional provider switching, two App/Auth installations, two standalone Files installations, and two App 0.1.0-to-0.2.0 upgrades, one per store. All **32 projects** resolve SDK 0.1.2 from npm/crates.io with registry integrity/checksum data. No SDK path patch or source archive substitute is used. Generated Auth consumers assert metadata and safe field issues before/after updates and provider switching. The reviewed App/Code Wasm comes from CLI commit `c78821993df005f0f46c0e2a81bfb75d5907cf66`; the native CLI is installed by the published installer. Current module source is supplied as the local author input, not mislabeled as a published Auth/Files release.

Both stores' upgrades retain an existing authenticated session and user host customizations. Files cases transfer 1,049,287 bytes through native streams, verify owner isolation and revocation, restart with persisted data, delete real content, compensate oversized/interrupted writes and preserve locked/offline preview behavior. Reports for all four hosted TS/Rust HTTP runtimes share an identical canonical public contract and observed effects after excluding only platform/runtime labels.

Coverage fixes in this delivery include adding Auth's direct serde_json dependency to generated Rust catalogs, building the new metadata fixture binary for integration, and removing the prerelease SDK override. The inherited upgrade command previously defaulted to SQLite despite the fresh-install matrix covering both stores. It now explicitly requests both stores; an eight-project-per-job gate rejects missing cases. Earlier superseded partial runs were canceled and are not counted as complete acceptance.

Module/Registry releases remain unchanged. Windows, physical devices, production deployments and LLM tool-selection quality are outside these checks. This batch does not add a Console UI or a standalone MCP service.

## Initial source validation

Local acceptance on 2026-11-15 (UTC+8), macOS arm64, Node.js 26.10.0, Rust 1.99.0 and disposable PostgreSQL 18. Tests use the reviewed SDK main commit `9d5edceb8d206b46b2c337ff77caebdc9927f8a8` in a temporary source consumer. These results do not establish compatibility with published SDK 0.1.1, which lacks the new APIs. This historical first run preceded the SDK release; it used a temporary SDK source override. Current validation uses the published registry SDK, as recorded above.

| Check | Result and scope |
| --- | --- |
| `npm test` in isolated consumer | 10 passed; existing Auth/Files regressions plus two metadata/native compatibility cases; strict build and emitted declarations included |
| `npm run lint` | Passed with warnings rejected |
| Standard `cargo check --all-targets` | Passed for native module source, examples/fixtures and tests |
| Standard `cargo test --locked` | 10 passed; one PostgreSQL case explicitly ignored in this run |
| Standard `cargo clippy --locked --all-targets -- -D warnings` | Passed without suppressions |
| Standard `cargo doc --locked --no-deps` and `cargo fmt --check` | Passed |
| Existing PostgreSQL TS/Rust tests | One TS case and one Rust case passed against a disposable PostgreSQL 18 database; the server was stopped and its temporary data removed |
| `npm run test:integration` | Five real-process HTTP cases passed, including new TS and Rust Auth/Files fixtures |
| Shared metadata resources/catalogs | JSON resources match their canonical contracts; catalog generation is deterministic |
| `git diff --check` | Passed |

[TypeScript](module-metadata-acceptance/typescript.json) and [Rust](module-metadata-acceptance/rust.json) reports are generated after fixture process/storage cleanup. They record actual SDK descriptions, public input failures, asserted checks, platform and observed effects. The [integration log](module-metadata-acceptance/integration.log) records all five HTTP cases. The [verification record](module-metadata-acceptance/verification.json) identifies the reviewed SDK and executed check scopes. The TS/Rust public reports match after excluding only the runtime label.

The added regressions establish metadata snapshot ownership, no discovery access to credential providers, namespaced IDs, private parser/unknown-key exclusion, unchanged direct Files calls, no stream pulls before authorization/name validation, native stream output, owner isolation, size-limit compensation and physical deletion. Existing regressions continue covering sessions/expiry/realm isolation, password hashing, failed writes/removals, acknowledgement loss, interrupted upload recovery, cancellation and symlink isolation.

The real-process fixtures register and log in users through HTTP, read an authenticated SDK-derived catalog, reject malformed/wrong-type credentials without leaking the fake private input, reject unsafe names/pages, compensate oversized uploads, round-trip native content and enforce ownership on read/delete. Each runtime observes two persisted accounts with hashed session storage, one successful upload, one delete and zero remaining files. Description reads produce no file operations; fixture metadata contains neither raw credentials/tokens nor storage paths.

The fixture is a thin native HTTP consumer, not an LLM or separate MCP transport. The initial local run did not cover full CLI installation or hosted platforms. Hosted source/CLI runs are recorded separately. Windows remains outside the verified scope. No Console UI, object-store provider or data migration is added.
