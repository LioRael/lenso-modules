# Auth/Files SDK metadata acceptance

Local acceptance on 2026-11-15 (UTC+8), macOS arm64, Node.js 26.10.0, Rust 1.99.0 and disposable PostgreSQL 18. Tests use the reviewed SDK main commit `9d5edceb8d206b46b2c337ff77caebdc9927f8a8` in a temporary source consumer. These results do not establish compatibility with published SDK 0.1.1, which lacks the new APIs. No module/SDK/CLI release or remote CI run is part of this batch.

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

The fixture is a thin native HTTP consumer, not an LLM or separate MCP transport. Generated v2 host descriptions and error projections are included in the source/templates, but full CLI-generated installation, provider switching and App upgrade workflows remain unrun for this batch. Windows, Linux, remote macOS CI and other PostgreSQL versions are unverified here. No Console UI, object-store provider or data migration is added.
