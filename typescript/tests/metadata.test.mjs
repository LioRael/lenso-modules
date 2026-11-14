import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile, mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Catalog, Executor } from '@lenso/app';
import { authModule } from '../dist/modules/auth/core.js';
import { Files, filesModule } from '../dist/modules/files/core.js';
import { SqliteFiles } from '../dist/modules/files/sqlite.js';
import { LocalBlobs } from '../dist/modules/files/local.js';
const declaration = async name => JSON.parse(await readFile(new URL(`../../contracts/${name}/metadata.json`, import.meta.url)));
const context = resources => ({ callId: 'metadata', resources, principal: { subject: 'alice', scopes: [] }, signal: new AbortController().signal });
test('Auth description uses SDK snapshots and does not inspect providers or credentials', async () => {
  const resources = { store: new Proxy({}, { get() { assert.fail('discovery touched store'); } }), passwords: new Proxy({}, { get() { assert.fail('discovery touched hasher'); } }), now: () => { assert.fail('discovery read clock'); }, sessionTtlMs: 60000 };
  const auth = authModule(resources, 'staff.auth');
  const expected = await declaration('auth');
  const catalog = new Catalog(); catalog.register(auth.module);
  assert.deepEqual(catalog.describe()[0].metadata, expected.module);
  assert.deepEqual(catalog.describeActions(), Object.entries(expected.actions).map(([name, metadata]) => ({ id: `staff.auth.${name}`, metadata })));
  const changed = catalog.describe(); changed[0].actionMetadata['staff.auth.login'].description = 'changed';
  assert.equal(catalog.describeActions()[1].metadata.description, expected.actions.login.description);
  for (const [input, path] of [[{ name: 1, password: 'private-password' }, ['name']], [{ name: 'Alice', password: 'short' }, ['password']], [{ name: 'Alice', password: 'private-password', 'private-key': true }, []]]) {
    await assert.rejects(new Executor(() => {}).callExternal(auth.actions.register, context(resources), input), error => {
      assert.equal(error.code, 'invalid_input'); assert.deepEqual(error.issues[0].path, path);
      assert.ok(!JSON.stringify(error).includes('private-')); return true;
    });
  }
});
test('Files SDK handles retain native streams, ownership, limits and legacy direct methods', async () => {
  const root = await mkdtemp(join(tmpdir(), 'lenso-metadata-'));
  const metadata = new SqliteFiles(join(root, 'files.sqlite'));
  try {
    await metadata.migrate(); const files = new Files(metadata, await LocalBlobs.open(root), 8);
    const module = filesModule(files, 'private.files'); const catalog = new Catalog(); catalog.register(module.module);
    const expected = await declaration('files');
    assert.deepEqual(catalog.describeActions(), Object.entries(expected.actions).map(([name, metadata]) => ({ id: `private.files.${name}`, metadata })));
    const executor = new Executor(() => {}), ctx = context(files);
    let pulled = false;
    const stream = () => (async function* () { pulled = true; yield Buffer.from('native'); })();
    await assert.rejects(executor.call(module.actions.upload, { ...ctx, principal: undefined }, { name: 'ok', source: stream() }), { code: 'unauthenticated' });
    assert.equal(pulled, false);
    await assert.rejects(executor.call(module.actions.upload, ctx, { name: '../private', source: stream() }), error => {
      assert.deepEqual(error.issues, [{ path: ['name'], code: 'invalid_value' }]); assert.ok(!JSON.stringify(error).includes('../private')); return true;
    });
    assert.equal(pulled, false);
    const file = await executor.call(module.actions.upload, ctx, { name: 'native', source: stream() });
    assert.equal((await files.stat(ctx.principal, file.id)).id, file.id);
    const download = await executor.call(module.actions.download, ctx, { id: file.id });
    const chunks = []; for await (const chunk of download.source) chunks.push(chunk);
    assert.equal(Buffer.concat(chunks).toString(), 'native');
    await assert.rejects(executor.call(module.actions.stat, { ...ctx, principal: { subject: 'bob', scopes: [] } }, { id: file.id }), { code: 'not_found' });
    await assert.rejects(files.list(ctx.principal, 'private-cursor', 0), error => {
      assert.deepEqual(error.issues, [{ path: ['after'], code: 'invalid_value' }, { path: ['limit'], code: 'out_of_range' }]); return true;
    });
    await assert.rejects(files.upload(ctx.principal, 'large', (async function* () { yield Buffer.alloc(9); })()), error => {
      assert.deepEqual(error.issues, [{ path: ['source'], code: 'out_of_range' }]); return true;
    });
    assert.equal((await files.list(ctx.principal)).length, 1);
    await executor.call(module.actions.delete, ctx, { id: file.id }); assert.equal((await files.list(ctx.principal)).length, 0);
  } finally { await metadata.close(); await rm(root, { recursive: true, force: true }); }
});
