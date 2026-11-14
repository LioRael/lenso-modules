import test from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtemp, rm, readFile, mkdir, writeFile, readdir } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { DatabaseSync } from 'node:sqlite';
import { describeActions } from '@lenso/app';
const root=resolve(import.meta.dirname,'..');
const declarations=Object.fromEntries(await Promise.all(['auth','files'].map(async name=>[name,JSON.parse(await readFile(join(root,'contracts',name,'metadata.json')))])));
for(const runtime of ['typescript','rust'])test(`${runtime}: SDK descriptions, safe input issues and real Auth/Files streams`,{timeout:120000},async()=>{
 const directory=await mkdtemp(join(tmpdir(),'lenso-module-metadata-'));
 const command=runtime==='typescript'?process.execPath:process.env.RUST_METADATA_BIN??join(root,'rust/target/debug/metadata-server');
 const args=runtime==='typescript'?[join(root,'typescript/dist/metadata-server.js')]:[];
 const child=spawn(command,args,{cwd:root,env:{...process.env,LENSO_METADATA_ROOT:directory},stdio:['ignore','pipe','pipe']});
 let errors='',report;
 child.stderr.on('data',data=>errors+=data);
 try {
  const port=await new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(new Error('Startup timeout: '+errors)),15000);child.once('error',error=>{clearTimeout(timer);reject(error);});child.once('exit',code=>{clearTimeout(timer);reject(new Error('Startup exit '+code+': '+errors));});child.stdout.on('data',data=>{const match=/LISTENING (\d+)/.exec(String(data));if(match){clearTimeout(timer);resolve(match[1]);}});});
  const url=`http://127.0.0.1:${port}`;
  async function call(path,{method='GET',body,token,raw}={}){
   const response=await fetch(url+path,{method,headers:{...(token?{authorization:'Bearer '+token}:{}),...(body?{'content-type':'application/json'}:{})},body:raw??(body?JSON.stringify(body):undefined),signal:AbortSignal.timeout(30000)});
   return {status:response.status,text:await response.text()};
  }
  assert.equal((await call('/manifest')).status,401);
  const input={name:'AliceUser',password:'private-password-value'};
  const invalid=await call('/auth/register',{method:'POST',body:{...input,name:17}});
  assert.equal(invalid.status,400);const invalidBody=JSON.parse(invalid.text);
  assert.deepEqual(invalidBody.issues,[{path:['name'],code:'invalid_value'}]);assert.ok(!invalid.text.includes('private-')&&!invalid.text.includes('cause')&&!invalid.text.includes('stack'));
  const unknown=await call('/auth/register',{method:'POST',body:{...input,'private-field':'private-value'}});assert.equal(unknown.status,400);assert.ok(!unknown.text.includes('private-'));assert.deepEqual(JSON.parse(unknown.text).issues[0].path,[]);
  const malformed=await call('/auth/register',{method:'POST',raw:'{"password":"private-secret'});assert.equal(malformed.status,400);assert.deepEqual(JSON.parse(malformed.text).issues,[{path:[],code:'invalid_format'}]);
  assert.equal((await call('/auth/register',{method:'POST',body:input})).status,200);
  const login=await call('/auth/login',{method:'POST',body:input});assert.equal(login.status,200);const token=JSON.parse(login.text).token;
  const manifest=await call('/manifest',{token});assert.equal(manifest.status,200);const description=JSON.parse(manifest.text);
  assert.deepEqual(describeActions(description.modules),description.actions);
  for(const module of description.modules){assert.deepEqual(module.metadata,declarations[module.id].module);assert.deepEqual(module.actionMetadata,Object.fromEntries(Object.entries(declarations[module.id].actions).map(([name,metadata])=>[`${module.id}.${name}`,metadata])));}
  assert.ok(!manifest.text.includes(token)&&!manifest.text.includes(input.password)&&!manifest.text.includes(directory));
  assert.equal(JSON.parse((await call('/files',{token})).text).files.length,0);
  assert.equal((await call('/files?name=no-auth',{method:'POST',raw:'bytes'})).status,401);
  const invalidName=await call('/files?name=..%2Fprivate-name',{method:'POST',raw:'bytes',token});assert.equal(invalidName.status,400);assert.deepEqual(JSON.parse(invalidName.text).issues,[{path:['name'],code:'invalid_value'}]);assert.ok(!invalidName.text.includes('private-name'));
  const invalidPage=await call('/files?after=private-cursor&limit=0',{token});assert.equal(invalidPage.status,400);assert.deepEqual(JSON.parse(invalidPage.text).issues,[{path:['after'],code:'invalid_value'},{path:['limit'],code:'out_of_range'}]);
  const oversized=await call('/files?name=large',{method:'POST',raw:'123456789',token});assert.equal(oversized.status,400);assert.deepEqual(JSON.parse(oversized.text).issues,[{path:['source'],code:'out_of_range'}]);
  assert.equal(JSON.parse((await call('/files',{token})).text).files.length,0);
  const upload=await call('/files?name=hello',{method:'POST',raw:'hello',token});assert.equal(upload.status,201);const file=JSON.parse(upload.text);
  assert.equal((await call(`/files/${file.id}/content`,{token})).text,'hello');assert.equal(JSON.parse((await call(`/files/${file.id}`,{token})).text).size,5);
  await call('/auth/register',{method:'POST',body:{...input,name:'OtherUser'}});const other=JSON.parse((await call('/auth/login',{method:'POST',body:{...input,name:'OtherUser'}})).text).token;
  assert.equal((await call(`/files/${file.id}`,{token:other})).status,404);assert.equal((await call(`/files/${file.id}`,{method:'DELETE',token:other})).status,404);
  assert.equal(JSON.parse((await call('/files',{token})).text).files.length,1);
  const db=new DatabaseSync(join(directory,'auth.sqlite'));try{assert.equal(db.prepare('SELECT count(*) AS n FROM lenso_auth_users').get().n,2);assert.ok(!JSON.stringify(db.prepare('SELECT * FROM lenso_auth_sessions').all()).includes(token));}finally{db.close();}
  assert.equal((await call(`/files/${file.id}`,{method:'DELETE',token})).status,200);assert.equal(JSON.parse((await call('/files',{token})).text).files.length,0);assert.equal((await readdir(join(directory,'files'))).length,0);
  report={schema_version:1,status:'passed',runtime,platform:{system:process.platform,machine:process.arch,node:process.version},description,invalidInput:invalidBody,checks:['SDK-derived Auth/Files descriptions','authenticated discovery without effects','private credentials, tokens and causes excluded','native streaming roundtrip','byte-limit compensation','owner isolation and delete','persisted accounts and hashed sessions'],effects:{accounts:2,uploads:1,deleted:1,remainingFiles:0}};
 }finally{if(child.exitCode===null&&child.signalCode===null){await new Promise(resolve=>{child.once('exit',resolve);child.kill('SIGTERM');});}await rm(directory,{recursive:true,force:true});}
 if(process.env.LENSO_MODULE_METADATA_REPORT_DIR){await mkdir(process.env.LENSO_MODULE_METADATA_REPORT_DIR,{recursive:true});await writeFile(join(process.env.LENSO_MODULE_METADATA_REPORT_DIR,`${runtime}.json`),JSON.stringify(report,null,2)+'\n');}
});
