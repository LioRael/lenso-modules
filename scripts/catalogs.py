"""Build complete language-specific catalogs from native source and assembly templates."""
from pathlib import Path
import json
root=Path(__file__).resolve().parent.parent
for lang in ('typescript','rust'):
    ext='ts' if lang=='typescript' else 'rs'
    runtime='node26' if lang=='typescript' else 'tokio'
    prefix='typescript/src' if lang=='typescript' else 'rust/src'
    modules={'auth':['core.ts','contracts.ts'] if lang=='typescript' else ['mod.rs'],
             'sqlite':['store.ts','schema.ts'] if lang=='typescript' else ['mod.rs','schema.sql'],
             'scrypt':['hasher.ts'] if lang=='typescript' else ['mod.rs']}
    body=f'''schema_version = 1
[modules.auth]
version = "0.1.0"
[modules.auth.supports]
language = ["{lang}"]
runtime = ["{runtime}"]
[modules.auth.options.ttl_ms]
prompt = "Session lifetime in milliseconds"
kind = {{ type = "integer", min = 60000, max = 2592000000 }}
default = 86400000
[modules.auth.slots.store]
contract = "lenso.auth-store"
version = "^0.1"
[modules.auth.slots.passwords]
contract = "lenso.password-hasher"
version = "^0.1"
[modules.sqlite]
version = "0.1.0"
[modules.sqlite.provides."lenso.auth-store"]
version = "0.1.0"
[modules.sqlite.options.path]
prompt = "SQLite database path"
kind = {{ type = "text" }}
default = "auth.sqlite"
[modules.scrypt]
version = "0.1.0"
[modules.scrypt.provides."lenso.password-hasher"]
version = "0.1.0"
[resources]
'''
    for module,files in modules.items():
        body+=module+' = ['+', '.join('{ path = '+json.dumps(f'{prefix}/modules/{module}/{name}')+', target = '+json.dumps(name)+' }' for name in files)+']\n'
    body+=f'''[generator]
schema_version = 1
directory = "generated/auth"
files = [{{scope="composition", template="templates/{lang}/assembly.{ext}.j2", target="{'index.ts' if lang=='typescript' else 'mod.rs'}"}}]
[samples.default]
instances = {{ auth = "auth" }}
context = {{language="{lang}", runtime="{runtime}"}}
[samples.multiple]
instances = {{ customer = "auth", staff = "auth" }}
context = {{language="{lang}", runtime="{runtime}"}}
'''
    deps= [('npm','@lenso/app','0.1.1','auth',{}),('npm','zod','^4','auth',{}),('npm','@types/node','^26','auth',{'section':'devDependencies'}),('npm','typescript','^6','auth',{'section':'devDependencies'})] if lang=='typescript' else [
      ('cargo','lenso-app','0.1.1','auth',{'features':['json']}),('cargo','serde','1','auth',{'features':['derive']}),('cargo','sha2','0.10','auth',{}),('cargo','getrandom','0.3','auth',{}),('cargo','rusqlite','0.42','sqlite',{'features':['bundled']}),('cargo','tokio','1','sqlite',{'features':['rt','sync']}),('cargo','tokio','1','scrypt',{'features':['rt','sync']}),('cargo','scrypt','0.11','scrypt',{}),('cargo','subtle','2','scrypt',{})]
    def literal(v):
      if isinstance(v,dict):return '{'+', '.join(k+'='+literal(x) for k,x in v.items())+'}'
      return json.dumps(v)
    for ecosystem,package,version,module,options in deps:
      body+='\n[[generator.dependencies]]\n'+f'ecosystem="{ecosystem}"\npackage="{package}"\nversion="{version}"\nmodule="{module}"\noptions='+literal(options)+'\n'
    (root/f'auth.{lang}.toml').write_text(body)
schema=(root/'contracts/auth/sqlite-v1.sql').read_text()
(root/'typescript/src/modules/sqlite/schema.ts').write_text('export const schema = '+json.dumps(schema)+';\n')
(root/'rust/src/modules/sqlite/schema.sql').write_text(schema)
