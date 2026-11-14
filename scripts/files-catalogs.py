"""Build complete Files catalogs from native implementations and transport assembly."""
from pathlib import Path
import json,tomllib
root=Path(__file__).resolve().parent.parent
for lang,runtime,ext in [('typescript','node26','ts'),('rust','tokio','rs')]:
 body=f'''schema_version=1
[modules.files]
version="0.1.0"
[modules.files.supports]
language=["{lang}"]
runtime=["{runtime}"]
app_host=["lenso-http-v2"]
[modules.files.options.max_bytes]
prompt="Maximum upload bytes"
kind={{type="integer",min=1,max=1073741824}}
default=52428800
[modules.files.slots.metadata]
contract="lenso.files-metadata"
version="^1"
[modules.files.slots.content]
contract="lenso.blob-store"
version="^1"
[modules.sqlite]
version="0.1.0"
provides={{"lenso.files-metadata"={{version="1.0.0"}}}}
[modules.sqlite.options.path]
prompt="Files SQLite database path"
kind={{type="text"}}
default="files.sqlite"
[modules.postgres]
version="0.1.0"
provides={{"lenso.files-metadata"={{version="1.0.0"}}}}
[modules.postgres.options.connection]
prompt="Files PostgreSQL environment binding"
kind={{type="environment"}}
default={{env="DATABASE_URL"}}
[modules.local]
version="0.1.0"
provides={{"lenso.blob-store"={{version="1.0.0"}}}}
[modules.local.options.directory]
prompt="Local content directory"
kind={{type="text"}}
default="data/files"
[resources]
'''
 resources={'files':['core.ts'],'local':['local.ts'],'sqlite':['sqlite.ts','sqlite-schema.ts'],'postgres':['postgres.ts','postgres-schema.ts']} if lang=='typescript' else {'files':['mod.rs'],'local':['local.rs'],'sqlite':['sqlite.rs','sqlite.sql'],'postgres':['postgres.rs','postgres.sql']}
 for module,files in resources.items():
  entries=[]
  for name in files:
   target='store.'+ext if name in ['sqlite.'+ext,'postgres.'+ext] else name
   entries.append('{path='+json.dumps(f'{lang}/src/modules/files/{name}')+',target='+json.dumps(target)+'}')
  body+=module+'=['+','.join(entries)+']\n'
 body+=f'''[generator]
schema_version=1
directory="generated/files"
files=[{{scope="composition",template="templates/{lang}/files.{ext}.j2",target="host.{ext}"}},{{scope="composition",template="templates/files-host.toml.j2",target="host.toml"}}]
[samples.default]
instances={{files="files"}}
bindings={{"files.metadata"={{type="module",module="sqlite"}}}}
context={{language="{lang}",runtime="{runtime}",app_host="lenso-http-v2"}}
[samples.postgres]
instances={{files="files"}}
bindings={{"files.metadata"={{type="module",module="postgres"}}}}
context={{language="{lang}",runtime="{runtime}",app_host="lenso-http-v2"}}
'''
 deps=[('npm','@lenso/app','0.1.1','files',{}),('npm','@types/node','^26','files',{'section':'devDependencies'}),('npm','pg','^8.16','postgres',{}),('npm','@types/pg','^8.15','postgres',{'section':'devDependencies'})] if lang=='typescript' else [
 ('cargo','lenso-app','0.1.1','files',{'features':['json']}),('cargo','serde','1','files',{'features':['derive']}),('cargo','serde_json','1','files',{}),('cargo','sha2','0.10','files',{}),('cargo','getrandom','0.3','files',{}),('cargo','axum','0.8','files',{}),('cargo','tokio','1','files',{'features':['fs','io-util','macros','net','rt','rt-multi-thread','signal','sync','time']}),('cargo','tokio-util','0.7','files',{'features':['io']}),('cargo','futures-util','0.3','files',{}),('cargo','cap-std','4','local',{}),('cargo','rusqlite','0.42','sqlite',{'features':['bundled']}),('cargo','tokio-postgres','0.7','postgres',{}),('cargo','postgres-native-tls','0.5','postgres',{}),('cargo','native-tls','0.2','postgres',{})]
 def literal(value):
  if isinstance(value,dict):return '{'+','.join(k+'='+literal(v) for k,v in value.items())+'}'
  return json.dumps(value)
 for ecosystem,package,version,module,options in deps:body+='\n[[generator.dependencies]]\n'+f'ecosystem="{ecosystem}"\npackage="{package}"\nversion="{version}"\nmodule="{module}"\noptions='+literal(options)+'\n'
 tomllib.loads(body);(root/f'files.{lang}.toml').write_text(body)
for db in ['sqlite','postgres']:
 schema=(root/f'contracts/files/{db}-v1.sql').read_text()
 (root/f'typescript/src/modules/files/{db}-schema.ts').write_text('export const schema = '+json.dumps(schema)+';\n')
 (root/f'rust/src/modules/files/{db}.sql').write_text(schema)
