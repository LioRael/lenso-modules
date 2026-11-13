#!/usr/bin/env python3
"""Verify Auth project configuration and adapter changes through real CLI consumers."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--binary', type=Path, required=True)
p.add_argument('--code-source', type=Path)
p.add_argument('--official', action='store_true')
p.add_argument('--local-modules', action='store_true', help='Use working-tree Auth with the published Code preset')
p.add_argument('--token-env')
p.add_argument('--out', type=Path, required=True)
p.add_argument('--language', choices=['typescript', 'rust'], required=True)
p.add_argument('--storage', choices=['sqlite', 'postgres'], default='sqlite')
a = p.parse_args()
if not a.official and not a.code_source:
    p.error('--official or --code-source is required')
if not os.environ.get('DATABASE_URL'):
    p.error('DATABASE_URL must point to a disposable PostgreSQL database for adapter switching')
root = Path(__file__).resolve().parent.parent
project = a.out.resolve()
project.mkdir(parents=True, exist_ok=False)
(project / 'src').mkdir()
local = not a.official or a.local_modules
if local:
    shutil.copytree(root / 'templates', project / 'author/templates')
    for lang in ['typescript', 'rust']:
        shutil.copytree(root / lang / 'src/modules', project / f'author/{lang}/src/modules')
        shutil.copyfile(root / f'auth.{lang}.toml', project / f'author/auth.{lang}.toml')
    shutil.copyfile(root / 'lenso.modules.toml', project / 'author/lenso.modules.toml')
if not a.official:
    code = project / 'lenso/extensions/code'
    code.mkdir(parents=True)
    for name in ['extension.toml', 'code.wasm']:
        shutil.copyfile(a.code_source.parent / name, code / name)
    (project / 'code.preset.toml').write_text('schema_version=1\nid="@lenso/code"\nversion="0.3.4"\nadopt="code/ready"\n[[extensions]]\nid="code"\nsource="lenso/extensions/code/extension.toml"\n')
if a.language == 'typescript':
    (project / 'package.json').write_text(json.dumps({'name': 'module-consumer', 'private': True, 'type': 'module',
        'imports': {'#modules/*': {'types': './src/features/*', 'default': './dist/features/*'}}}))
    shutil.copyfile(root / 'typescript/tsconfig.json', project / 'tsconfig.json')
    entry = project / 'src/main.ts'
else:
    (project / 'Cargo.toml').write_text('[package]\nname="auth-consumer"\nversion="0.1.0"\nedition="2024"\n[workspace]\n')
    (project / 'src/lib.rs').write_text('#[path="features/auth/assembly/mod.rs"]\npub mod generated;\n')
    entry = project / 'src/main.rs'

def entry_for(storage):
    text = (root / ('tests/consumers/typescript.ts' if a.language == 'typescript' else 'tests/consumers/rust.rs')).read_text()
    text = text.replace('./generated/auth/assembly/index.js', '#modules/auth/assembly/index.js')
    text = text.replace('i_61757468("consumer.sqlite")', 'i_61757468()')
    text = text.replace('i_61757468(Some(std::path::Path::new("consumer.sqlite")))?',
                        'i_61757468(None).await?' if storage == 'postgres' else 'i_61757468(None)?')
    return text.replace('new_current_thread().build()', 'new_current_thread().enable_all().build()')

entry.write_text(entry_for(a.storage))
env = dict(os.environ, LENSO_CACHE_DIR=str(project / 'cache'), AUTH_DATABASE_URL=os.environ['DATABASE_URL'])
# Share native dependencies across these sequential fixtures, not application state.
env['CARGO_TARGET_DIR'] = str(project.parent / 'native-target')
commands = []

def run(*args, expected=0):
    r = subprocess.run([str(a.binary.resolve()), '--project', str(project), *args, '--json'], env=env,
                       capture_output=True, text=True, timeout=300)
    assert env['DATABASE_URL'] not in r.stdout + r.stderr, 'database connection leaked in CLI output'
    if a.token_env and env.get(a.token_env):
        assert env[a.token_env] not in r.stdout + r.stderr, 'GitHub credential leaked'
    assert r.returncode == expected, (args, r.stdout[-4000:], r.stderr[-2000:])
    assert '\x1b' not in r.stdout
    commands.append({'args': list(args), 'exit_code': r.returncode})
    return json.loads(r.stdout)

creds = ['--token-env', a.token_env] if a.token_env else []
setup = ['--input', json.dumps({'author_root': 'author', 'format': 'lenso-modules-v1', 'catalog': 'lenso.modules.toml'})] if local else []
run('init', '--preset', 'code' if a.official else 'code.preset.toml', *setup, '--yes', *creds)
lock = json.loads((project / 'lenso.lock').read_text())
code_version = next(x for x in lock['extensions'] if x['id'] == 'code')['definition']['version']
assert (0, 3, 4) <= tuple(map(int, code_version.split('.'))) < (0, 4, 0), code_version
assert any(x['name'] == 'auth' for x in run('code', 'search', 'auth')['outputs']['search']['results'])
choice = run('code', 'add', 'auth', '--yes', expected=2)
assert choice['question']['id'].startswith('code:module-language:')
choice = run('code', 'preview', 'auth', '--language', a.language, '--yes', expected=2)
assert set(choice['question']['kind']['options']) == {'module:postgres', 'module:sqlite'}
settings = {'project': {'language': a.language, 'runtime': 'node26' if a.language == 'typescript' else 'tokio'},
    'paths': {'modules': 'src/features', 'import_alias': '#modules'},
    'defaults': {'providers': {'lenso.auth-store': a.storage}},
    'modules': {'auth': {'options': {'ttl_ms': 120000}}, 'sqlite': {'options': {'path': 'configured.sqlite'}},
                'postgres': {'options': {'connection': {'env': 'AUTH_DATABASE_URL'}}}}}
pending = run('code', 'config', '--input', json.dumps({'settings': settings}), expected=2)
assert not (project / 'lenso.code.toml').exists()
run('code', 'config', '--input', json.dumps({'settings': settings}), '--yes')
config_bytes = (project / 'lenso.code.toml').read_bytes()
preview = run('code', 'preview', 'auth')['outputs']['resolve']
assert preview['module']['version'] == '0.2.1'
assert not (project / 'src/code.modules.json').exists()
pending = run('code', 'add', 'auth', expected=2)
answers = project / 'decline.json'
answers.write_text(json.dumps({pending['question']['id']: False}))
run('code', 'add', 'auth', '--answers', str(answers), expected=130)
assert not (project / 'src/features').exists()
run('code', 'add', 'auth', '--yes')
assert (project / 'lenso.code.toml').read_bytes() == config_bytes
index = json.loads((project / 'src/code.modules.json').read_text())
assert index['language'] == a.language
assembly = project / ('src/features/auth/assembly/index.ts' if a.language == 'typescript' else 'src/features/auth/assembly/mod.rs')
core = project / ('src/features/auth/modules/auth/core.ts' if a.language == 'typescript' else 'src/features/auth/modules/auth/mod.rs')
guide = project / 'src/features/auth/assembly/README.md'
assert '#modules/auth/assembly/index.js' in guide.read_text() if a.language == 'typescript' else 'src/features/auth/assembly/mod.rs' in guide.read_text()
assert '120000' in assembly.read_text() and '120000 ms' in guide.read_text()

native = [['npm', 'exec', '--', 'tsc', '-p', 'tsconfig.json'], ['node', 'dist/main.js']] if a.language == 'typescript' else [[os.environ.get('CARGO', 'cargo'), 'run', '--release', '--quiet']]
def execute():
    for cmd in native:
        r = subprocess.run(cmd, cwd=project, env=env, capture_output=True, text=True, timeout=600)
        assert r.returncode == 0, (cmd, r.stderr[-5000:])
    assert 'AUTH_CONSUMER_OK' in r.stdout
    return next(line.split('=', 1)[1] for line in r.stdout.splitlines() if line.startswith('AUTH_USER_ID='))

user_id = execute()
if a.storage == 'sqlite':
    assert (project / 'configured.sqlite').is_file()
else:
    assert 'AUTH_DATABASE_URL' in assembly.read_text()
assert run('code', 'preview', 'auth', '--locked', '--offline')['outputs']['resolve']['files'] == []
wrong = 'rust' if a.language == 'typescript' else 'typescript'
run('code', 'add', 'auth', '--language', wrong, '--yes', expected=1)
core.write_text('// user customization\n' + core.read_text())
if local:
    source = project / ('author/typescript/src/modules/auth/core.ts' if a.language == 'typescript' else 'author/rust/src/modules/auth/mod.rs')
    source.write_text(source.read_text() + '\n// author update\n')
run('code', 'update', 'auth', '--yes')
assert 'user customization' in core.read_text()
if local:
    assert 'author update' in core.read_text()

other = 'sqlite' if a.storage == 'postgres' else 'postgres'
run('code', 'config', '--provider', 'lenso.auth-store=' + other, '--yes')
assert run('code', 'preview', 'auth')['outputs']['resolve']['files'] == []
assert a.storage in guide.read_text()
# A project preference is not permission to replace an installed store.
switch = json.dumps({'bindings': {'auth.store': {'type': 'module', 'module': other}}})
manifest = project / ('package.json' if a.language == 'typescript' else 'Cargo.toml')
before = {str(f): f.read_bytes() for f in [entry, manifest, core, assembly]}
sqlite_data = (project / 'configured.sqlite').read_bytes() if a.storage == 'sqlite' else None
plan = run('code', 'preview', 'auth', '--input', switch)['outputs']['resolve']
assert any('/modules/' + a.storage + '/' in f for f in plan['deletions'])
assert all(Path(f).read_bytes() == content for f, content in before.items())
pending = run('code', 'add', 'auth', '--input', switch, expected=2)
answers.write_text(json.dumps({pending['question']['id']: False}))
run('code', 'add', 'auth', '--input', switch, '--answers', str(answers), expected=130)
assert all(Path(f).read_bytes() == content for f, content in before.items())
run('code', 'add', 'auth', '--input', switch, '--yes')
assert entry.read_bytes() == before[str(entry)]
assert 'user customization' in core.read_text() and '120000' in assembly.read_text()
assert not any(f.is_file() for f in (project / 'src/features/auth/modules' / a.storage).rglob('*'))
assert (project / 'src/features/auth/modules' / other).is_dir()
assert ('tokio-postgres' in manifest.read_text() if a.language == 'rust' else '"pg"' in manifest.read_text()) == (other == 'postgres')
if a.language == 'rust':
    assert ('rusqlite' in manifest.read_text()) == (other == 'sqlite')
if sqlite_data is not None:
    assert (project / 'configured.sqlite').read_bytes() == sqlite_data
else:
    probe = subprocess.run(['node', '--input-type=module', '-e', '''import { Pool } from "pg";
const db=new Pool({connectionString:process.env.AUTH_DATABASE_URL});
try { const r=await db.query("SELECT id FROM lenso_auth_users WHERE realm=$1 AND id=$2",["auth",process.argv[1]]); if(r.rowCount!==1)throw new Error("PostgreSQL data disappeared"); }
finally { await db.end(); }''', user_id], cwd=root, env=env, capture_output=True, text=True, timeout=30)
    assert probe.returncode == 0, probe.stderr
entry.write_text(entry_for(other))
execute()
run('code', 'update', 'auth', '--yes')
assert run('code', 'preview', 'auth', '--locked', '--offline')['outputs']['resolve']['files'] == []
for file in [project / 'lenso.code.toml', project / 'lenso.toml', project / 'lenso.lock', project / 'src/code/modules/lenso/auth.json']:
    assert env['DATABASE_URL'] not in file.read_text()
if local:
    directory = project / 'author/lenso.modules.toml'
    original = directory.read_text()
    # Copy the entire latest release, including its language targets.
    start = original.rfind('[[modules.auth.releases]]\n')
    release = original[start:]
    directory.write_text(original + '\n' + release.replace('0.2.1', '0.2.2'))
    assert run('code', 'preview', 'auth')['outputs']['resolve']['module']['version'] == '0.2.1'
    run('code', 'update', 'auth', '--yes')
    assert run('code', 'preview', '@lenso/auth')['outputs']['resolve']['module']['version'] == '0.2.2'
    directory.write_text(directory.read_text().replace('reference = "v0.2.2"', 'reference = "moved-tag"'))
    run('code', 'update', 'auth', '--yes', expected=1)
(project / 'verification.json').write_text(json.dumps({'language': a.language, 'storage': a.storage, 'switched_to': other,
    'official': a.official, 'local_modules': local, 'code_version': code_version, 'project_config': True, 'compiled_and_ran': True,
    'preview_denial_update_offline': True, 'native_cleanup': True, 'data_preserved': True, 'commands': commands}, indent=2) + '\n')
print(a.language + '/' + a.storage + ' Auth project configuration and switch passed', flush=True)
