#!/usr/bin/env python3
"""Compile/run generated native consumers through published CLI 1.0 and the Auth preset."""
import argparse,json,os,shutil,subprocess,tempfile
from pathlib import Path
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary',type=Path,required=True)
parser.add_argument('--code-source',help='Local Code manifest for pre-publication verification')
parser.add_argument('--official',action='store_true')
parser.add_argument('--out',type=Path)
parser.add_argument('--token-env')
args=parser.parse_args()
if not args.official and not args.code_source:parser.error('--code-source or --official is required')
root=Path(__file__).resolve().parent.parent
binary=args.binary.resolve(strict=True)
out=args.out.resolve() if args.out else Path(tempfile.mkdtemp(prefix='lenso-auth-consumers-'))
if args.out:out.mkdir(parents=True,exist_ok=False)
reports=[]
for language in ['typescript','rust']:
 project=out/language;project.mkdir()
 (project/'src').mkdir()
 if language=='typescript':
  (project/'package.json').write_text(json.dumps({'name':'auth-consumer','private':True,'type':'module'}))
  shutil.copyfile(root/'typescript/tsconfig.json',project/'tsconfig.json')
  (project/'src/main.ts').write_text('''import { Executor } from "@lenso/app";
import { i_61757468 } from "./generated/auth/assembly/index.js";
const app=i_61757468(":memory:");app.store.migrate();
const executor=new Executor(()=>{});
const context={callId:"consumer",principal:undefined,resources:app.resources,signal:new AbortController().signal};
const user=await executor.call(app.actions.register,context,{name:"Alice",password:"correct horse battery"});
const session=await executor.call(app.actions.login,context,{name:"alice",password:"correct horse battery"});
const principal=await app.principal(session.token);
if(principal?.subject!==user.id)throw new Error("identity mismatch");
await executor.call(app.actions.logout,{...context,principal},{token:session.token});
if(await app.principal(session.token))throw new Error("revocation failed");
app.store.close();console.log("AUTH_CONSUMER_OK");
''')
 else:
  (project/'Cargo.toml').write_text('[package]\nname="auth-consumer"\nversion="0.1.0"\nedition="2024"\n[workspace]\n')
  (project/'src/lib.rs').write_text('#[path="generated/auth/assembly/mod.rs"]\npub mod generated;\n')
  (project/'src/main.rs').write_text('''use auth_consumer::generated::{i_61757468,auth::Credentials};
use lenso_app::{Executor,Context,CancellationToken,CallRecord};
fn main()->Result<(),Box<dyn std::error::Error>>{
 tokio::runtime::Builder::new_current_thread().build()?.block_on(async {
 let app=i_61757468(Some(std::path::Path::new(":memory:")))?;app.store.migrate().await?;
 let executor=Executor::new(|_:CallRecord|{});
 let mut context=Context{call_id:"consumer".into(),principal:None,resources:app.resources,cancellation:CancellationToken::new()};
 let user=executor.call(&app.actions.register,&context,Credentials{name:"Alice".into(),password:"correct horse battery".into()}).await?;
 let session=executor.call(&app.actions.login,&context,Credentials{name:"alice".into(),password:"correct horse battery".into()}).await?;
 context.principal=context.resources.principal(Some(&session.token)).await?;
 assert_eq!(context.principal.as_ref().unwrap().subject,user.id);
 executor.call(&app.actions.logout,&context,auth_consumer::generated::auth::TokenInput{token:session.token.clone()}).await?;
 assert!(context.resources.principal(Some(&session.token)).await?.is_none());println!("AUTH_CONSUMER_OK");Ok(())
 })
}
''')
 env=dict(os.environ,LENSO_CACHE_DIR=str(out/('cache-'+language)))
 creds=['--token-env',args.token_env] if args.token_env else []
 if args.official:preset='auth'
 else:
  author=project/'author';author.mkdir()
  for lang in ['typescript','rust']:
   shutil.copytree(root/lang/'src/modules',author/lang/'src/modules')
   shutil.copyfile(root/f'auth.{lang}.toml',author/f'auth.{lang}.toml')
  shutil.copytree(root/'templates',author/'templates')
  local_code=project/'lenso/extensions/code';local_code.mkdir(parents=True)
  code_path=Path(args.code_source).resolve()
  shutil.copyfile(code_path,local_code/'extension.toml')
  shutil.copyfile(code_path.parent/'code.wasm',local_code/'code.wasm')
  preset_path=project/'auth.preset.toml'
  preset_path.write_text('''schema_version=1
id="@lenso/auth"
version="0.1.0"
create="code/ready"
adopt="code/ready"
[[extensions]]
id="code"
source='''+json.dumps('lenso/extensions/code/extension.toml')+'''
version="^0.3.0"
[extensions.options.setup]
author_root="author"
target="src"
[extensions.options.variants.typescript]
runtime="node26"
[extensions.options.variants.typescript.setup]
catalog="auth.typescript.toml"
native={npm={manifest="package.json"}}
[extensions.options.variants.rust]
runtime="tokio"
[extensions.options.variants.rust.setup]
catalog="auth.rust.toml"
native={cargo={manifest="Cargo.toml"}}
''')
  preset='auth.preset.toml'
 def run(*arguments,expected=0):
  p=subprocess.run([str(binary),'--project',str(project),*map(str,arguments),'--json'],env=env,text=True,capture_output=True,timeout=300)
  assert p.returncode==expected,(language,arguments,p.returncode,p.stdout[-5000:],p.stderr[-3000:])
  return json.loads(p.stdout)
 pending=run('init','--preset',preset,'--yes',*creds,expected=2)
 assert pending['question']['id'].startswith('code:language:')
 assert not (project/'src/generated').exists()
 run('init','--preset',preset,'--input',json.dumps({'language':language}),'--yes',*creds)
 config=run('config')['outputs']['main']['config']['cli']
 selected=next(x for x in config['extensions'] if x['id']=='code')
 assert selected['options']['selection']['language']==language
 assert run('code','validate')['outputs']['validate']['validated']
 run('code','compose-preview','--sample','default')
 assert not (project/'src/generated').exists()
 pending=run('code','compose','--sample','default',expected=2)
 answers=project/'decline.json';answers.write_text(json.dumps({pending['question']['id']:False}))
 run('code','compose','--sample','default','--answers',answers,expected=130)
 assert not (project/'src/generated').exists()
 run('code','compose','--sample','default','--yes')
 if language=='typescript':
  commands=[['npm','exec','--','tsc','-p','tsconfig.json'],['node','dist/main.js']]
 else:commands=[[os.environ.get('CARGO','cargo'),'run','--release','--quiet']]
 for command in commands:
  completed=subprocess.run(command,cwd=project,env=env,capture_output=True,text=True,timeout=300)
  assert completed.returncode==0,(command,completed.stdout[-3000:],completed.stderr[-4000:])
 assert 'AUTH_CONSUMER_OK' in completed.stdout
 before=(project/'lenso.toml').read_bytes()
 failed=run('ext','configure','code','--input',json.dumps({'language':'rust' if language=='typescript' else 'typescript'}),'--yes',*creds,expected=1)
 assert 'new project target' in failed['message']
 assert (project/'lenso.toml').read_bytes()==before
 failed=run('ext','configure','code','--input',json.dumps({'language':language,'runtime':'browser'}),'--yes',*creds,expected=1)
 assert 'unsupported runtime' in failed['message']
 assert (project/'lenso.toml').read_bytes()==before
 failed=run('code','compose-preview','--input',json.dumps({'context':{'language':'go'}}),expected=1)
 assert 'conflicts with the installed target' in failed['message']
 core=project/('src/generated/auth/modules/auth/core.ts' if language=='typescript' else 'src/generated/auth/modules/auth/mod.rs')
 core.write_text('// local application customization\n'+core.read_text())
 if not args.official:
  source=project/('author/typescript/src/modules/auth/core.ts' if language=='typescript' else 'author/rust/src/modules/auth/mod.rs')
  source.write_text(source.read_text()+'\n// upstream module revision\n')
  preview=run('code','compose-preview')
  assert preview['outputs']['compose']['summary']['merged']==1
  run('code','compose','--yes')
  assert 'local application customization' in core.read_text() and 'upstream module revision' in core.read_text()
 replay=run('code','compose-preview','--locked','--offline')
 assert replay['outputs']['compose']['files']==[]
 reports.append({'language':language,'selection':selected['options']['selection'],'compiled_and_ran':True,'language_change_rejected':True,'local_customization_preserved':True,'preview_and_consent':True,'locked_offline':True})
 print(language+' published-SDK generated consumer passed',flush=True)
(out/'verification.json').write_text(json.dumps({'official':args.official,'cli_version':subprocess.check_output([str(binary),'--version'],text=True).strip(),'cases':reports},indent=2)+'\n')
print('Auth CLI acceptance passed: '+str(out/'verification.json'),flush=True)
