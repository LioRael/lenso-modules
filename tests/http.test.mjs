import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtempSync, rmSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
const root=resolve(import.meta.dirname,'..');
const credential=JSON.parse(readFileSync(join(root,'contracts/auth/behavior.json'))).valid[0];
async function start(t,language,db,migrate=true){
 const command=language==='typescript'?process.execPath:(process.env.RUST_AUTH_BIN??join(root,'rust/target/release/auth-server'));
 const args=language==='typescript'?[join(root,'typescript/dist/server.js')]:[];
 if(migrate)args.push('--migrate');
 const child=spawn(command,args,{cwd:root,env:{PATH:process.env.PATH,PORT:'0',LENSO_AUTH_DB:db},stdio:['ignore','pipe','pipe']});
 let errors='';child.stderr.on('data',b=>errors+=b);
 t.after(async()=>{if(child.exitCode===null){child.kill('SIGTERM');await new Promise(r=>child.once('exit',r));}});
 const port=await new Promise((ok,fail)=>{const timer=setTimeout(()=>{child.kill('SIGKILL');fail(new Error('Server startup timeout: '+errors));},15000);child.on('exit',c=>{clearTimeout(timer);fail(new Error('Server exited '+c+': '+errors));});child.stdout.on('data',b=>{const m=/LISTENING (\d+)/.exec(String(b));if(m){clearTimeout(timer);ok(Number(m[1]));}});});
 const call=async(method,path,body,token)=>{
  const headers={};if(body!==undefined)headers['Content-Type']='application/json';if(token)headers.Authorization='Bearer '+token;
  const response=await fetch(`http://127.0.0.1:${port}${path}`,{method,headers,body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(30000)});
  return {status:response.status,body:await response.json(),cache:response.headers.get('cache-control')};
 };
 return {call,child,port};
}
for(const language of ['typescript','rust'])test(language+' HTTP lifecycle, authorization and body limits',{timeout:180000},async t=>{
 const dir=mkdtempSync(join(tmpdir(),'lenso-auth-http-'));t.after(()=>rmSync(dir,{recursive:true,force:true}));
 const app=await start(t,language,join(dir,'auth.sqlite'));
 assert.equal((await app.call('GET','/auth/session')).status,401);
 const created=await app.call('POST','/auth/register',credential);assert.equal(created.status,200);assert.equal(created.body.name,credential.name.toLowerCase());
 assert.equal((await app.call('POST','/auth/register',{...credential,name:credential.name.toLowerCase()})).status,409);
 assert.equal((await app.call('POST','/auth/register',{...credential,subject:'spoofed'})).status,400);
 const wrong=await app.call('POST','/auth/login',{name:credential.name,password:'incorrect password'});const missing=await app.call('POST','/auth/login',{name:'missing',password:'incorrect password'});assert.equal(wrong.status,401);assert.deepEqual(wrong.body,missing.body);
 const session=await app.call('POST','/auth/login',credential);assert.equal(session.status,200);assert.match(session.body.token,/^[0-9a-f]{64}$/);
 const current=await app.call('GET','/auth/session',undefined,session.body.token);assert.deepEqual(current.body,created.body);assert.equal(current.cache,'no-store');
 assert.equal((await app.call('POST','/auth/logout',undefined,session.body.token)).status,200);assert.equal((await app.call('GET','/auth/session',undefined,session.body.token)).status,401);
 assert.equal((await app.call('POST','/auth/register',{name:'valid',password:'x'.repeat(5000)})).status,413);
 assert.equal((await app.call('GET','/unknown')).status,404);
});
test('TS/Rust reopen the same SQLite accounts and sessions and enforce concurrent uniqueness',{timeout:180000},async t=>{
 const dir=mkdtempSync(join(tmpdir(),'lenso-auth-interop-'));t.after(()=>rmSync(dir,{recursive:true,force:true}));const db=join(dir,'auth.sqlite');
 const ts=await start(t,'typescript',db);const rust=await start(t,'rust',db,false);
 const outcomes=await Promise.all([ts.call('POST','/auth/register',credential),rust.call('POST','/auth/register',{...credential,name:credential.name.toUpperCase()})]);assert.deepEqual(outcomes.map(r=>r.status).sort(),[200,409]);
 const fromTS=await ts.call('POST','/auth/login',credential);assert.equal(fromTS.status,200);assert.equal((await rust.call('GET','/auth/session',undefined,fromTS.body.token)).status,200);
 assert.equal((await rust.call('POST','/auth/logout',undefined,fromTS.body.token)).status,200);assert.equal((await ts.call('GET','/auth/session',undefined,fromTS.body.token)).status,401);
 const fromRust=await rust.call('POST','/auth/login',credential);assert.equal(fromRust.status,200);assert.equal((await ts.call('GET','/auth/session',undefined,fromRust.body.token)).status,200);
});
