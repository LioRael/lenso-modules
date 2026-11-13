#!/usr/bin/env python3
"""Exercise Code module discovery, choices, consent, generation and native execution."""
import argparse,json,os,shutil,subprocess
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--binary',type=Path,required=True)
p.add_argument('--code-source',type=Path)
p.add_argument('--official',action='store_true')
p.add_argument('--token-env')
p.add_argument('--out',type=Path,required=True)
p.add_argument('--language',choices=['typescript','rust'],required=True)
p.add_argument('--storage',choices=['sqlite','postgres'],default='sqlite')
a=p.parse_args();root=Path(__file__).resolve().parent.parent
project=a.out.resolve();project.mkdir(parents=True,exist_ok=False);(project/'src').mkdir()
shutil.copytree(root/'templates',project/'author/templates')
for lang in ['typescript','rust']:
 shutil.copytree(root/lang/'src/modules',project/f'author/{lang}/src/modules')
 shutil.copyfile(root/f'auth.{lang}.toml',project/f'author/auth.{lang}.toml')
shutil.copyfile(root/'lenso.modules.toml',project/'author/lenso.modules.toml')
if not a.official:
 code=project/'lenso/extensions/code';code.mkdir(parents=True)
 for name in ['extension.toml','code.wasm']:shutil.copyfile(a.code_source.parent/name,code/name)
 (project/'code.preset.toml').write_text('''schema_version=1
 id="@lenso/code"
 version="0.3.2"
 adopt="code/ready"
 [[extensions]]
 id="code"
 source="lenso/extensions/code/extension.toml"
 ''')
if a.language=='typescript':
 (project/'package.json').write_text('{"name":"module-consumer","private":true,"type":"module"}')
 shutil.copyfile(root/'typescript/tsconfig.json',project/'tsconfig.json')
 entry=project/'src/main.ts';entry.write_text((root/'tests/consumers/typescript.ts').read_text())
else:
 (project/'Cargo.toml').write_text('[package]\nname="auth-consumer"\nversion="0.1.0"\nedition="2024"\n[workspace]\n')
 (project/'src/lib.rs').write_text('#[path="generated/auth/assembly/mod.rs"]\npub mod generated;\n')
 entry=project/'src/main.rs';entry.write_text((root/'tests/consumers/rust.rs').read_text())
if a.storage=='postgres':
 assert os.environ.get('DATABASE_URL'),'DATABASE_URL required'
 entry.write_text(entry.read_text().replace('i_61757468("consumer.sqlite")','i_61757468()').replace('i_61757468(Some(std::path::Path::new("consumer.sqlite")))?','i_61757468(None).await?').replace('new_current_thread().build()','new_current_thread().enable_all().build()'))
env=dict(os.environ,LENSO_CACHE_DIR=str(project/'cache'))
def run(*args,expected=0):
 r=subprocess.run([str(a.binary.resolve()),'--project',str(project),*args,'--json'],env=env,capture_output=True,text=True,timeout=300)
 assert r.returncode==expected,(args,r.stdout[-4000:],r.stderr[-2000:]);assert '\x1b' not in r.stdout
 return json.loads(r.stdout)
creds=['--token-env',a.token_env] if a.token_env else []
if a.official:run('init','--preset','code','--yes',*creds)
else:run('init','--preset','code.preset.toml','--input',json.dumps({'author_root':'author','format':'lenso-modules-v1','catalog':'lenso.modules.toml'}),'--yes')
assert any(x['name']=='auth' for x in run('code','search','auth')['outputs']['search']['results'])
choice=run('code','add','auth','--yes',expected=2)
assert choice['question']['id'].startswith('code:module-language:')
choice=run('code','preview','auth','--language',a.language,'--yes',expected=2)
assert set(choice['question']['kind']['options'])=={'module:postgres','module:sqlite'}
intent=json.dumps({'bindings':{'auth.store':{'type':'module','module':a.storage}}})
run('code','preview','auth','--language',a.language,'--input',intent)
assert not (project/'src/code.modules.json').exists()
pending=run('code','add','auth','--language',a.language,'--input',intent,expected=2)
answers=project/'decline.json';answers.write_text(json.dumps({pending['question']['id']:False}))
run('code','add','auth','--language',a.language,'--input',intent,'--answers',str(answers),expected=130)
assert not (project/'src/generated').exists()
run('code','add','auth','--language',a.language,'--input',intent,'--yes')
index=json.loads((project/'src/code.modules.json').read_text());assert index['language']==a.language
commands=[['npm','exec','--','tsc','-p','tsconfig.json'],['node','dist/main.js']] if a.language=='typescript' else [[os.environ.get('CARGO','cargo'),'run','--release','--quiet']]
for cmd in commands:
 r=subprocess.run(cmd,cwd=project,env=env,capture_output=True,text=True,timeout=300);assert r.returncode==0,(cmd,r.stderr[-5000:])
assert 'AUTH_CONSUMER_OK' in r.stdout
preview=run('code','preview','auth','--locked','--offline')['outputs']['resolve'];assert preview['files']==[]
wrong='rust' if a.language=='typescript' else 'typescript'
assert 'new project target' in run('code','add','auth','--language',wrong,'--yes',expected=1)['message']
core=project/('src/generated/auth/modules/auth/core.ts' if a.language=='typescript' else 'src/generated/auth/modules/auth/mod.rs')
core.write_text('// user customization\n'+core.read_text())
source=project/('author/typescript/src/modules/auth/core.ts' if a.language=='typescript' else 'author/rust/src/modules/auth/mod.rs')
if not a.official:source.write_text(source.read_text()+'\n// author update\n')
run('code','update','auth','--yes');assert 'user customization' in core.read_text()
if not a.official:assert 'author update' in core.read_text()
if not a.official:
 directory=project/'author/lenso.modules.toml'
 original=directory.read_text();release=original[original.index('[[modules.auth.releases]]'):]
 directory.write_text(original+'\n'+release.replace('0.2.0','0.2.1'))
 assert run('code','preview','auth')['outputs']['resolve']['module']['version']=='0.2.0'
 run('code','update','auth','--yes')
 assert run('code','preview','@lenso/auth')['outputs']['resolve']['module']['version']=='0.2.1'
 directory.write_text(directory.read_text().replace('reference = "v0.2.1"','reference = "moved-tag"'))
 assert 'without a new version' in run('code','update','auth','--yes',expected=1)['message']
(project/'verification.json').write_text(json.dumps({'language':a.language,'storage':a.storage,'entry':'code add auth','compiled_and_ran':True,'questions_consent_preview_offline_update':True},indent=2)+'\n')
print(a.language+'/'+a.storage+' Code module consumer passed')
