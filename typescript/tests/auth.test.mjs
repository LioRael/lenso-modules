import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, rmSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { Executor } from "@lenso/app";
import { SqliteAuthStore } from "../dist/modules/sqlite/store.js";
import { ScryptHasher } from "../dist/modules/scrypt/hasher.js";
import { authModule, tokenHash } from "../dist/modules/auth/core.js";
const behavior = JSON.parse(readFileSync(new URL("../../contracts/auth/behavior.json", import.meta.url)));
function fixture(t, realm="auth", file) {
 const dir=mkdtempSync(join(tmpdir(),"lenso-auth-"));const filename=file??join(dir,"db.sqlite");
 const store=new SqliteAuthStore(filename,realm);store.migrate();let clock=behavior.clockMs;
 const auth=authModule({store,passwords:new ScryptHasher(),now:()=>clock,sessionTtlMs:behavior.sessionTtlMs},realm);
 t.after(()=>{store.close();rmSync(dir,{recursive:true,force:true});});
 const executor=new Executor(()=>{});
 const call=(name,input,principal)=>executor.callExternal(auth.actions[name],{callId:"test",resources:auth.resources,principal,signal:new AbortController().signal},input);
 return {auth,store,call,filename,clock:(v)=>clock=v};
}
test("shared credentials fixtures validate native/external Actions",async t=>{
 const f=fixture(t);
 for(const input of behavior.invalid) await assert.rejects(f.call("register",input),{code:"invalid_input"});
 for(const input of behavior.valid){const user=await f.call("register",input);assert.equal(user.name,input.name.toLowerCase());assert.deepEqual(Object.keys(user).sort(),["id","name"]);}
 await assert.rejects(f.call("register",{...behavior.valid[0],subject:"attacker"}),{code:"invalid_input"});
});
test("login, expiry and revocation; raw secrets are not persisted",async t=>{
 const f=fixture(t);const input=behavior.valid[0];const user=await f.call("register",input);
 await assert.rejects(f.call("register",{...input,name:input.name.toLowerCase()}),{code:"conflict"});
 const session=await f.call("login",input);assert.match(session.token,/^[0-9a-f]{64}$/);
 const db=new DatabaseSync(f.filename);t.after(()=>db.close());
 const stored=db.prepare("SELECT token_hash FROM lenso_auth_sessions").get();assert.equal(stored.token_hash,tokenHash(session.token));assert.notEqual(stored.token_hash,session.token);
 const credential=db.prepare("SELECT password_hash FROM lenso_auth_users").get();assert(!credential.password_hash.includes(input.password));
 const principal=await f.auth.principal(session.token);assert.equal(principal.subject,user.id);assert.deepEqual(principal.scopes,[]);
 assert.deepEqual(await f.call("session",null,principal),user);
 await assert.rejects(f.call("session",null),{code:"unauthenticated"});
 await f.call("logout",{token:session.token},principal);assert.equal(await f.auth.principal(session.token),undefined);
 const second=await f.call("login",input);f.clock(second.expiresMs);assert.equal(await f.auth.principal(second.token),undefined);
});
test("unknown and incorrect passwords have the same public failure",async t=>{
 const f=fixture(t);await f.call("register",behavior.valid[0]);
 for(const name of [behavior.valid[0].name,"missing"]){await assert.rejects(f.call("login",{name,password:"incorrect password"}),{code:"unauthenticated",message:"Invalid credentials"});}
 await assert.rejects(f.auth.resources.passwords.verify("password", "scrypt$v1$999999999$8$1$bad"),{code:"internal"});
});
test("independent realms cannot adopt or revoke one another's sessions",async t=>{
 const a=fixture(t,"customer");const b=fixture(t,"staff",a.filename);const input=behavior.valid[0];
 const userA=await a.call("register",input);const userB=await b.call("register",input);assert.notEqual(userA.id,userB.id);
 const session=await a.call("login",input);assert.equal(await b.auth.principal(session.token),undefined);
 await assert.rejects(b.call("logout",{token:session.token},{subject:userB.id,scopes:[]}),{code:"unauthenticated"});assert(await a.auth.principal(session.token));
});
test("two SQLite connections enforce normalized uniqueness and restart persistence",async t=>{
 const a=fixture(t);const b=fixture(t,"auth",a.filename);const input=behavior.valid[0];
 const outcomes=await Promise.allSettled([a.call("register",input),b.call("register",{...input,name:input.name.toUpperCase()})]);
 assert.equal(outcomes.filter(x=>x.status==="fulfilled").length,1);assert.equal(outcomes.find(x=>x.status==="rejected").reason.code,"conflict");
 const session=await a.call("login",input);assert(await b.auth.principal(session.token));
});
test("migration is explicit and rejects a newer schema without deleting records",async t=>{
 const f=fixture(t);await f.call("register",behavior.valid[0]);const db=new DatabaseSync(f.filename);t.after(()=>db.close());
 db.exec("INSERT INTO lenso_auth_migrations VALUES(2)");assert.throws(()=>f.store.migrate(),{code:"internal"});
 assert.equal(db.prepare("SELECT COUNT(*) AS n FROM lenso_auth_users").get().n,1);
 assert.equal(db.prepare("SELECT MAX(version) AS v FROM lenso_auth_migrations").get().v,2);
});
