import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, rm, readdir, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Files } from '../dist/modules/files/core.js';
import { SqliteFiles } from '../dist/modules/files/sqlite.js';
import { LocalBlobs } from '../dist/modules/files/local.js';
const owner={subject:'alice',scopes:[]},other={subject:'bob',scopes:[]};
const input=async function*(){yield Buffer.from('first');yield Buffer.from('second');};
test('scoped files, limits, failed writes and delete retries',async()=>{
 const root=await mkdtemp(join(tmpdir(),'lenso-files-'));
 const metadata=new SqliteFiles(join(root,'metadata.sqlite'));
 try {
  await metadata.migrate();const blobs=await LocalBlobs.open(root);const files=new Files(metadata,blobs,20);
  let pulled=false;
  await assert.rejects(files.upload(undefined,'secret', (async function*(){pulled=true;yield Buffer.alloc(1);})()),{code:'unauthenticated'});assert.equal(pulled,false);
  const file=await files.upload(owner,'résumé.bin',input());
  await assert.rejects(files.stat(other,file.id),{code:'not_found'});await assert.rejects(files.delete(other,file.id),{code:'not_found'});
  const download=await files.download(owner,file.id);const chunks=[];for await(const chunk of download.source)chunks.push(chunk);assert.equal(Buffer.concat(chunks).toString(),'firstsecond');
  await assert.rejects(files.upload(owner,'large',(async function*(){yield Buffer.alloc(21);})()),{code:'invalid_input'});
  assert.equal((await files.list(owner)).length,1);assert.equal((await readdir(join(root,'files'))).length,1);
  const activate=metadata.activate.bind(metadata);metadata.activate=async()=>{throw new Error('simulated commit failure');};
  await assert.rejects(files.upload(owner,'broken',input()));metadata.activate=activate;
  assert.equal((await readdir(join(root,'files'))).length,1);
  let fail=true;const remove=blobs.remove.bind(blobs);blobs.remove=async id=>{if(fail)throw new Error('temporary storage failure');await remove(id);};
  await assert.rejects(files.delete(owner,file.id));assert.equal((await files.list(owner)).length,0);
  fail=false;await files.delete(owner,file.id);assert.equal((await readdir(join(root,'files'))).length,0);
  const interrupted={id:'b'.repeat(32),owner:'alice',name:'interrupted',size:0,sha256:'',created_ms:Date.now()};
  await metadata.reserve(interrupted);await blobs.write(interrupted.id,input());
  await metadata.reserve({...interrupted,id:'c'.repeat(32),owner:'bob'});
  assert.equal(await files.recoverStoppedUploads(owner),1);assert.equal((await readdir(join(root,'files'))).length,0);
  assert.equal((await metadata.retired('bob')).length,1);
  metadata.activate=async(...args)=>{await activate(...args);throw new Error('commit acknowledgement lost');};
  await assert.rejects(files.upload(owner,'committed',input()));
  const committed=await files.list(owner);assert.equal(committed.length,1);
  const source=(await files.download(owner,committed[0].id)).source;const result=[];for await(const chunk of source)result.push(chunk);assert.equal(Buffer.concat(result).toString(),'firstsecond');
 } finally {await metadata.close();await rm(root,{recursive:true,force:true});}
});
test('local content keys cannot read symlinks outside storage',async()=>{
 const root=await mkdtemp(join(tmpdir(),'lenso-file-path-'));
 try {
  const blobs=await LocalBlobs.open(root);await writeFile(join(root,'secret'),'outside');
  await symlink(join(root,'secret'),join(root,'files','a'.repeat(32)));
  await assert.rejects(blobs.read('a'.repeat(32)));await assert.rejects(blobs.read('../secret'));
 } finally {await rm(root,{recursive:true,force:true});}
});
