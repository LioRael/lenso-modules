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
parser.add_argument('--storage',choices=['sqlite','postgres','both'],default='sqlite')
parser.add_argument('--language',choices=['typescript','rust'])
args=parser.parse_args()
if not args.official and not args.code_source:parser.error('--code-source or --official is required')
root=Path(__file__).resolve().parent.parent
binary=args.binary.resolve(strict=True)
out=args.out.resolve() if args.out else Path(tempfile.mkdtemp(prefix='lenso-auth-consumers-'))
if args.out:out.mkdir(parents=True,exist_ok=False)
reports=[]
for language,storage in [(lang,store) for lang in ([args.language] if args.language else ['typescript','rust']) for store in (['sqlite','postgres'] if args.storage=='both' else [args.storage])]:
 project=out/(language+'-'+storage);project.mkdir()
 (project/'src').mkdir()
 if language=='typescript':
  (project/'package.json').write_text(json.dumps({'name':'auth-consumer','private':True,'type':'module'}))
  shutil.copyfile(root/'typescript/tsconfig.json',project/'tsconfig.json')
  (project/'src/main.ts').write_text('''import { Executor } from "@lenso/app";
import { i_61757468 } from "./generated/auth/assembly/index.js";
const app=i_61757468("consumer.sqlite");await app.store.migrate();
const executor=new Executor(()=>{});
const context={callId:"consumer",principal:undefined,resources:app.resources,signal:new AbortController().signal};
const user=await executor.call(app.actions.register,context,{name:"consumer_"+process.pid,password:"correct horse battery"});
const session=await executor.call(app.actions.login,context,{name:"consumer_"+process.pid,password:"correct horse battery"});
const principal=await app.principal(session.token);
if(principal?.subject!==user.id)throw new Error("identity mismatch");
await executor.call(app.actions.logout,{...context,principal},{token:session.token});
if(await app.principal(session.token))throw new Error("revocation failed");
await app.store.close();console.log("AUTH_USER_ID="+user.id);console.log("AUTH_CONSUMER_OK");
''')
 else:
  (project/'Cargo.toml').write_text('[package]\nname="auth-consumer"\nversion="0.1.0"\nedition="2024"\n[workspace]\n')
  (project/'src/lib.rs').write_text('#[path="generated/auth/assembly/mod.rs"]\npub mod generated;\n')
  (project/'src/main.rs').write_text('''use auth_consumer::generated::{i_61757468,auth::Credentials};
use lenso_app::{Executor,Context,CancellationToken,CallRecord};
fn main()->Result<(),Box<dyn std::error::Error>>{
 tokio::runtime::Builder::new_current_thread().build()?.block_on(async {
 let app=i_61757468(Some(std::path::Path::new("consumer.sqlite")))?;app.store.migrate().await?;
 let executor=Executor::new(|_:CallRecord|{});
 let mut context=Context{call_id:"consumer".into(),principal:None,resources:app.resources,cancellation:CancellationToken::new()};
 let user=executor.call(&app.actions.register,&context,Credentials{name:format!("consumer_{}",std::process::id()),password:"correct horse battery".into()}).await?;
 let session=executor.call(&app.actions.login,&context,Credentials{name:format!("consumer_{}",std::process::id()),password:"correct horse battery".into()}).await?;
 context.principal=context.resources.principal(Some(&session.token)).await?;
 assert_eq!(context.principal.as_ref().unwrap().subject,user.id);
 executor.call(&app.actions.logout,&context,auth_consumer::generated::auth::TokenInput{token:session.token.clone()}).await?;
 assert!(context.resources.principal(Some(&session.token)).await?.is_none());println!("AUTH_USER_ID={}",user.id);println!("AUTH_CONSUMER_OK");Ok(())
 })
}
''')
 if storage=='postgres':
  if not os.environ.get('DATABASE_URL'):parser.error('DATABASE_URL is required for PostgreSQL acceptance')
  entry=project/('src/main.ts' if language=='typescript' else 'src/main.rs')
  text=entry.read_text().replace('i_61757468("consumer.sqlite")','i_61757468()').replace('i_61757468(Some(std::path::Path::new("consumer.sqlite")))?','i_61757468(None).await?')
  entry.write_text(text.replace('new_current_thread().build()','new_current_thread().enable_all().build()'))
 env=dict(os.environ,LENSO_CACHE_DIR=str(out/('cache-'+language+'-'+storage)))
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
  assert not env.get('DATABASE_URL') or env['DATABASE_URL'] not in p.stdout+p.stderr, 'database connection leaked in CLI output'
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
 sample='postgres' if storage=='postgres' else 'default'
 intent=json.dumps({'instances':{'auth':'auth'}})
 choice=run('code','compose-preview','--input',intent,'--yes',expected=2)
 assert set(choice['question']['kind']['options'])=={'module:sqlite','module:postgres'}
 answer=project/'provider.json';answer.write_text(json.dumps({choice['question']['id']:'module:'+storage}))
 chosen=run('code','compose-preview','--input',intent,'--answers',answer)
 assert chosen['outputs']['compose']['files']
 assert not (project/'src/generated').exists()
 run('code','compose-preview','--sample',sample)
 assert not (project/'src/generated').exists()
 pending=run('code','compose','--sample',sample,expected=2)
 answers=project/'decline.json';answers.write_text(json.dumps({pending['question']['id']:False}))
 run('code','compose','--sample',sample,'--answers',answers,expected=130)
 assert not (project/'src/generated').exists()
 run('code','compose','--sample',sample,'--yes')
 if language=='typescript':
  commands=[['npm','exec','--','tsc','-p','tsconfig.json'],['node','dist/main.js']]
 else:commands=[[os.environ.get('CARGO','cargo'),'run','--release','--quiet']]
 for command in commands:
  completed=subprocess.run(command,cwd=project,env=env,capture_output=True,text=True,timeout=300)
  assert completed.returncode==0,(command,completed.stdout[-3000:],completed.stderr[-4000:])
 assert 'AUTH_CONSUMER_OK' in completed.stdout
 user_id=next(line.split('=',1)[1] for line in completed.stdout.splitlines() if line.startswith('AUTH_USER_ID='))
 manifest=(project/('package.json' if language=='typescript' else 'Cargo.toml')).read_text()
 assert ('tokio-postgres' in manifest if language=='rust' else '"pg"' in manifest)==(storage=='postgres')
 assert (project/'src/generated/auth/modules'/storage).is_dir()
 assert not (project/'src/generated/auth/modules'/('sqlite' if storage=='postgres' else 'postgres')).exists()
 for generated in [project/'lenso.toml',project/'lenso.lock',project/'src/composition.json']:
  assert os.environ.get('DATABASE_URL','UNSET_DATABASE_URL') not in generated.read_text()
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
 # Changing providers must reconcile files/dependencies without touching the application or data.
 other='sqlite' if storage=='postgres' else 'postgres'
 switch=json.dumps({'bindings':{'auth.store':{'type':'module','module':other}},'configuration':{}})
 old_entry=(project/('src/main.ts' if language=='typescript' else 'src/main.rs')).read_text()
 sqlite_data=(project/'consumer.sqlite').read_bytes() if storage=='sqlite' else None
 before_manifest=(project/('package.json' if language=='typescript' else 'Cargo.toml')).read_bytes()
 preview=run('code','compose-preview','--input',switch)['outputs']['compose']
 assert any('/modules/'+storage+'/' in path for path in preview['deletions'])
 assert (project/('package.json' if language=='typescript' else 'Cargo.toml')).read_bytes()==before_manifest
 assert (project/'src/generated/auth/modules'/storage).is_dir()
 run('code','compose','--input',switch,'--yes')
 assert not any(p.is_file() for p in (project/'src/generated/auth/modules'/storage).rglob('*'))
 assert (project/'src/generated/auth/modules'/other).is_dir()
 assert 'local application customization' in core.read_text()
 entry=project/('src/main.ts' if language=='typescript' else 'src/main.rs')
 assert entry.read_text()==old_entry
 if sqlite_data is not None:assert (project/'consumer.sqlite').read_bytes()==sqlite_data
 else:
  probe=subprocess.run(['node','--input-type=module','-e','''import { Pool } from "pg";
const db=new Pool({connectionString:process.env.DATABASE_URL});
try { const result=await db.query("SELECT id FROM lenso_auth_users WHERE realm=$1 AND id=$2",["auth",process.argv[1]]); if(result.rowCount!==1)throw new Error("PostgreSQL user data disappeared after switching adapters"); }
finally {await db.end();}''',user_id],cwd=root,env=env,capture_output=True,text=True,timeout=30)
  assert probe.returncode==0,probe.stderr
 manifest=(project/('package.json' if language=='typescript' else 'Cargo.toml')).read_text()
 assert ('tokio-postgres' in manifest if language=='rust' else '"pg"' in manifest)==(other=='postgres')
 if other=='postgres':
  adjusted=old_entry.replace('i_61757468("consumer.sqlite")','i_61757468()').replace('i_61757468(Some(std::path::Path::new("consumer.sqlite")))?','i_61757468(None).await?').replace('new_current_thread().build()','new_current_thread().enable_all().build()')
 else:
  adjusted=old_entry.replace('i_61757468()','i_61757468("consumer.sqlite")').replace('i_61757468(None).await?','i_61757468(Some(std::path::Path::new("consumer.sqlite")))?')
 entry.write_text(adjusted)
 for command in commands:
  completed=subprocess.run(command,cwd=project,env=env,capture_output=True,text=True,timeout=300)
  assert completed.returncode==0,(command,completed.stdout[-3000:],completed.stderr[-4000:])
 assert 'AUTH_CONSUMER_OK' in completed.stdout
 reports.append({'language' :language,'storage':storage,'switched_to':other,'data_preserved':True,'selection':selected['options']['selection'],'compiled_and_ran':True,'language_change_rejected':True,'local_customization_preserved':True,'preview_and_consent':True,'locked_offline':True})
 print(language+'/'+storage+' published-SDK generated consumer passed',flush=True)
(out/'verification.json').write_text(json.dumps({'official':args.official,'cli_version':subprocess.check_output([str(binary),'--version'],text=True).strip(),'cases':reports},indent=2)+'\n')
print('Auth CLI acceptance passed: '+str(out/'verification.json'),flush=True)
