# Optional Auth/Files descriptions and input issues

This is unreleased module source for the SDK metadata contract at `LioRael/lenso` commit `9d5edceb8d206b46b2c337ff77caebdc9927f8a8`. Published SDK 0.1.1 does not provide the required metadata/error APIs. Existing release tags and their published dependency declarations are unchanged. Do not release these module catalogs against SDK 0.1.1: publish a compatible SDK first, then update the source-consumer and generated dependency declarations together. Ordinary checks against the old installed SDK cannot validate these additions.

## Descriptions

Auth's existing native Actions now declare optional SDK metadata for registration, login, current-session lookup and logout. `authModule(...).module.describe()` in TypeScript and `Actions::describe::<Store,Hasher>()` in Rust include the annotations. Discovery reads static public data; it does not access credentials, providers, environment values or the clock.

Files keeps all existing native methods. TypeScript adds `filesModule(files, id?)`, returning `module`, `actions` and the existing Files instance as `resources`. Rust adds `files::Actions::new(id)` and `describe::<Metadata,Blobs>(id)`. Upload, list, stat, download and delete are thin SDK Actions over the same native methods. Use SDK Executor to invoke these optional handles. Existing callers can continue using Files directly.

```ts
const extension = filesModule(files, "private.files");
catalog.register(extension.module);
const file = await executor.call(extension.actions.upload, context, {
  name: "notes.txt", source: nativeByteStream,
});
```

Here `context.resources` is the existing Files instance, and `context.principal` comes from trusted authentication. TypeScript uses the existing AbortSignal. Rust upload accepts `UploadInput::new(name, Reader)`; its owned mutex satisfies the SDK's input Sync bound without imposing Sync on the native reader. The Action takes the reader out once and passes it directly to Files. Download returns the existing native reader. No byte buffering, JSON conversion, forced serde bounds or transport-specific execution registry is introduced into the native stream API.

The public [Auth](../contracts/auth/metadata.json) and [Files](../contracts/files/metadata.json) declarations contain operation/field documentation, advisory input/output schemas, capabilities and effects. They are copied into each language's installable source resources by the existing catalog scripts. Generated catalogs include these JSON files, so installed sources do not depend on paths back into this repository. Rust deserializes the static annotation into SDK ActionMetadata/ModuleMetadata; TypeScript supplies it directly to SDK definitions. Actual IDs always come from native Actions, including namespaced instances.

Module versions are not invented from the catalogs' different historical version numbers. Declared capabilities describe provider needs, not permission grants. Read operations explicitly declare `effects: []`; mutation effects describe intent, not observed outcomes. Upload's input schema describes the transport-owned name projection; the source remains a native stream. Download deliberately has no JSON output schema. These declarations do not make streaming Actions callable through `callExternal`/`call_json` without an application-owned codec/stream transport.

The generated v2 hosts expose native `describe()` functions alongside their existing host contract. They do not add a public discovery route or expand management access. HTTP, Console or Agent hosts can reuse the SDK Module/Catalog descriptions under their own discovery policy. The acceptance host has an authenticated description endpoint; it is a fixture, not a new public Auth/Files API or a Console/MCP server.

## Input errors and compatibility

Automatic SDK decoding/parser failures preserve private causes and expose only declared public paths. Auth domain validation additionally reports `name`, `password` or `token` with `invalid_value`, including native Rust calls. TS and Rust retain their existing public messages and native validation, username normalization, UTF-8 password byte bounds and unknown-field rejection. Rust missing-field failures can still have root paths; no private parser text is parsed to guess a field.

Files name failures report `name/invalid_value`; invalid cursors and page bounds report `after/invalid_value` and `limit/out_of_range`; upload size failures report `source/out_of_range`. These contain field names, never received filenames, cursors, credentials or stream bytes. The original byte limit, pending-state compensation and delete retry behavior are retained. Invalid or foreign file IDs continue returning `not_found`; metadata does not grant ownership or change that enumeration boundary. Configuration and framework-owned transport errors can still be root failures.

Reference and generated Action error adapters serialize SDK `toJSON()`/`public()`, preserving the existing flat `{code,message}` wire shape and adding optional `issues`. Internal errors retain their fixed host message. Causes and stacks remain native. Auth JSON decoding additionally returns a root `invalid_format` issue. The outer App host still owns body-limit/timeouts and other framework rejections; this change does not claim every transport failure has a field path.

SDK schemas are annotations. Native parser/invariant logic and Access remain authoritative. Explicit application messages/issues are author-owned public data; the SDK does not scrub an arbitrary application debug dump. No actual passwords, bearer tokens, storage paths, hashes from live credentials or provider connection strings belong in metadata.

## Reproduce validation without changing release dependencies

With Node.js 26, Rust 1.99 and the reviewed SDK checkout (with its npm dependencies installed):

```sh
python3 scripts/verify-sdk-metadata.py --sdk /path/to/lenso
```

The script copies module sources to a temporary consumer, builds/packs the reviewed SDK, installs its App archive without saving manifests/locks, and applies an invocation-local Cargo crates.io patch inside that consumer. All standard Rust checks use the installed native Cargo with empty wrappers. No global Cargo configuration or project release dependencies are changed. Results and per-command logs are written under ignored `.local/module-metadata`; temporary consumers are removed. The isolated dependency lock may resolve the SDK's additional optional dependency; it is never written back to the module repository.

Set `AUTH_POSTGRES_URL` to a disposable database to also run the existing TS/Rust PostgreSQL acceptance. The normal Rust suite explicitly skips its environment-dependent PostgreSQL test; the extra pass runs it with `--ignored`. Generated CLI install/upgrade workflows and remote Linux/macOS CI are separate checks and have not been run for this source batch. Their source-consumer jobs must use the same reviewed SDK override until a compatible registry release exists. Published-only consumers must continue using the existing release tags during that interval.

See the [local acceptance record](module-metadata-acceptance.md) for results and limits.
