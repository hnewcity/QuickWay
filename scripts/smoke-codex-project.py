"""Real Codex + QuickWay project and resume acceptance (uses cloud quota)."""
import argparse
import copy
import http.server
import importlib.util
import json
import os
import pathlib
import queue
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.request

spec = importlib.util.spec_from_file_location('project_checks', pathlib.Path(__file__).with_name('smoke-claude-project.py'))
checks = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checks)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--base', required=True)
    parser.add_argument('--key-file', required=True)
    parser.add_argument('--model', default='quick-smart')
    parser.add_argument('--restart-cloud-before-result', action='store_true')
    args = parser.parse_args()
    os.environ['PYTHONUTF8'] = '1'
    os.environ['PYTHONIOENCODING'] = 'utf-8'
    executable = shutil.which('codex')
    if not executable and os.name == 'nt':
        candidates = list((pathlib.Path(os.environ['LOCALAPPDATA'])/'OpenAI/Codex/bin').glob('*/codex.exe'))
        if candidates:
            executable = str(max(candidates, key=lambda p: p.stat().st_mtime))
    assert executable, 'Codex CLI was not found'
    key = pathlib.Path(args.key_file).read_text().strip()
    root = pathlib.Path(tempfile.mkdtemp(prefix='quickway-codex-project-'))
    project = root/'project'
    project.mkdir()
    report = {'project': str(project), 'requests': [], 'tool_calls': [], 'stages': [], 'errors': [],
              'version': subprocess.check_output([executable, '--version'], text=True).strip()}
    print(json.dumps({'project': str(project), 'version': report['version']}), flush=True)
    bundled = json.loads(subprocess.check_output([executable, 'debug', 'models', '--bundled'], text=True, encoding='utf-8'))
    template = next((m for m in bundled['models'] if m.get('visibility') == 'list'), bundled['models'][0])
    model = copy.deepcopy(template)
    model.update(slug=args.model, display_name='Quick Smart (cloud)', description='Quick Smart via QuickWay',
                 priority=1000, context_window=200000, max_context_window=200000)
    for field in ('upgrade', 'availability_nux', 'auto_compact_token_limit'):
        model.pop(field, None)
    model['base_instructions'] = re.sub(r'You are Codex, an agent based on [^.]+\.',
                                        'You are Codex, an agent based on Quick Smart (cloud).', model.get('base_instructions', ''), count=1)
    bundled['models'] = [m for m in bundled['models'] if m['slug'] != args.model] + [model]
    catalog = root/'codex-models.json'
    catalog.write_text(json.dumps(bundled), encoding='utf-8')

    class Proxy(http.server.BaseHTTPRequestHandler):
        def log_message(self, *args): pass
        def do_GET(self): self.forward(None)
        def do_POST(self): self.forward(self.rfile.read(int(self.headers.get('content-length', 0))))
        def forward(self, body):
            if body and self.path.startswith('/v1/responses'):
                data = json.loads(body)
                if (args.restart_cloud_before_result and not report.get('forced_cloud_restart')
                    and (project/'notes.py').is_file() and (project/'README.md').is_file()
                    and any(project.glob('test_*.py')) and any(
                    item.get('type') in ('function_call_output', 'custom_tool_call_output') for item in data.get('input', [])
                )):
                    restart = urllib.request.Request(args.base+'/__smoke/restart-cloud', data=b'', headers={'x-api-key': key})
                    with urllib.request.urlopen(restart, timeout=60) as stopped:
                        assert stopped.status == 204
                    report['forced_cloud_restart'] = True
                    report['restart_after_files_created'] = sorted(p.name for p in project.glob('*.py'))
                    print(json.dumps({'forced_cloud_restart_after_tool_execution': True}), flush=True)
                declared = list(data.get('tools', []))
                for item in data.get('input', []):
                    if item.get('type') == 'additional_tools': declared.extend(item.get('tools', []))
                def flatten(tools):
                    for tool in tools:
                        if tool.get('type') == 'namespace': yield from flatten(tool.get('tools', []))
                        else: yield tool
                declared = list(flatten(declared))
                shape = {'items': len(data.get('input', [])), 'tools': len(declared),
                         'input_types': [i.get('type', i.get('role')) for i in data.get('input', [])[-5:]],
                         'tool_types': sorted(set(t.get('type', '') for t in declared))}
                report['requests'].append(shape)
                print(json.dumps({'request': shape}), flush=True)
            headers = {k: v for k, v in self.headers.items() if k.lower() not in
                       ('host', 'content-length', 'connection', 'authorization', 'x-api-key')}
            headers['Authorization'] = 'Bearer ' + key
            request = urllib.request.Request(args.base+self.path, data=body, headers=headers)
            try:
                response = urllib.request.urlopen(request, timeout=190)
            except urllib.error.HTTPError as error:
                response = error
            self.send_response(response.status)
            self.send_header('Content-Type', response.headers.get('Content-Type', 'application/json'))
            self.send_header('Connection', 'close')
            self.end_headers()
            pending = b''
            try:
                with response:
                    while chunk := response.read1(65536):
                        self.wfile.write(chunk)
                        self.wfile.flush()
                        pending += chunk
                        while b'\n' in pending:
                            line, pending = pending.split(b'\n', 1)
                            if not line.startswith(b'data: ') or line[6:] == b'[DONE]': continue
                            event = json.loads(line[6:])
                            if event.get('type') in ('error', 'response.failed'):
                                report['errors'].append(event)
                                print(json.dumps({'api_error': event}), flush=True)
                            if event.get('type') == 'response.output_item.done':
                                item = event['item']
                                if item.get('type') in ('function_call', 'custom_tool_call'):
                                    call = {k: item.get(k) for k in ('type', 'name', 'namespace')}
                                    report['tool_calls'].append(call)
                                    print(json.dumps({'tool': call}), flush=True)
                    if response.status >= 400:
                        error = {'status': response.status, 'body': pending.decode('utf-8', errors='replace')[:1600]}
                        report['errors'].append(error)
                        print(json.dumps({'api_error': error}), flush=True)
            except (BrokenPipeError, ConnectionResetError): pass

    proxy = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Proxy)
    threading.Thread(target=proxy.serve_forever, daemon=True).start()
    env = os.environ.copy()
    env['QUICKWAY_API_KEY'] = key
    overrides = ["model_provider='quickway'", "model_providers.quickway.name='QuickWay'",
                 f"model_providers.quickway.base_url='http://127.0.0.1:{proxy.server_port}/v1'",
                 "model_providers.quickway.wire_api='responses'", "model_providers.quickway.env_key='QUICKWAY_API_KEY'",
                 "model_providers.quickway.http_headers.x-quickway-thinking-effort='low'",
                 "model_reasoning_effort='low'", f"model_catalog_json='{catalog.as_posix()}'", "approval_policy='never'"]

    def run_stage(prompt, session_id=None):
        cli = ['exec']
        if session_id:
            cli += ['resume', '--skip-git-repo-check', '--json']
        else:
            cli += ['--skip-git-repo-check', '-s', 'danger-full-access', '--json']
        cli += ['-m', args.model]
        for override in overrides: cli += ['-c', override]
        if session_id: cli.append(session_id)
        cli.append('-')
        # Same PowerShell argument transport as the Windows desktop launcher.
        if os.name == 'nt':
            env.update(QUICKWAY_CODEX_EXE=executable, QUICKWAY_CODEX_ARGS='\n'.join(cli))
            command = ['powershell.exe', '-NoLogo', '-NoProfile', '-Command',
                       '& $env:QUICKWAY_CODEX_EXE @($env:QUICKWAY_CODEX_ARGS -split "`n")']
        else: command = [executable, *cli]
        child = subprocess.Popen(command, cwd=project, env=env, stdin=subprocess.PIPE,
                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding='utf-8', errors='replace')
        child.stdin.write(prompt)
        child.stdin.close()
        lines = queue.Queue()
        def read(stream):
            for line in stream: lines.put(line)
        threading.Thread(target=read, args=(child.stdout,), daemon=True).start()
        threading.Thread(target=read, args=(child.stderr,), daemon=True).start()
        stage = {'events': [], 'stderr': [], 'thread': session_id}
        report['stages'].append(stage)
        deadline = time.monotonic()+720
        try:
            while time.monotonic() < deadline:
                try: line = lines.get(timeout=1)
                except queue.Empty:
                    if child.poll() is not None: break
                    continue
                if not line.startswith('{'):
                    stage['stderr'].append(line[-1200:])
                    continue
                event = json.loads(line)
                stage['events'].append(event)
                if event.get('type') == 'thread.started': stage['thread'] = event['thread_id']
                if event.get('type') in ('turn.completed', 'turn.failed', 'error'):
                    print(json.dumps({'codex': event}), flush=True)
                if event.get('type') == 'item.completed':
                    item = event.get('item', {})
                    print(json.dumps({'item': item.get('type'), 'status': item.get('status')}), flush=True)
                    if item.get('type') == 'agent_message':
                        assert '<decision question=' not in item.get('text', ''), 'Native markup leaked'
                        assert not re.search(r'(?m)^\s*⟨assistant msg-[0-9a-f-]+ \| ', item.get('text', '')), 'Native assistant reference leaked'
            else: raise AssertionError('Codex stage timed out')
            child.wait(timeout=15)
            stage['exit'] = child.returncode
            assert child.returncode == 0, stage
            assert any(e.get('type') == 'turn.completed' for e in stage['events']), stage
            assert not any(e.get('type') == 'turn.failed' or (e.get('type') == 'error' and not e.get('message', '').startswith('Reconnecting...'))
                           for e in stage['events']), stage
            assert not any('Model metadata for' in line for line in stage['stderr']), stage['stderr']
            return stage['thread']
        finally:
            if child.poll() is None:
                child.terminate()
                child.wait(timeout=15)

    try:
        session = run_stage('Build a complete Python standard-library project in this empty directory. Work only here; '
                            'no network, external files, git, or delegation. Create notes.py, README.md, and at least six '
                            'meaningful unittest tests. CLI: python notes.py --file PATH add TEXT appends a nonempty note '
                            'to a UTF-8 JSON array of strings. list prints the array as JSON; list --upper uppercases '
                            'output without changing storage. Missing storage means []. Empty notes and corrupt/non-array '
                            'storage exit 2 with useful stderr, no traceback, and preserve existing data. Support Unicode. '
                            'Use your file patch tool to write the files. Run all tests and fix failures until passing. '
                            'Do not just describe code: actually create and run the project. Finish with a brief summary.')
        assert (project/'notes.py').is_file() and session
        original_tests = {p.name: p.read_bytes() for p in project.glob('test*.py')}
        assert original_tests, 'Codex did not create project tests'
        (project/'test_acceptance.py').write_text(checks.REGRESSION, encoding='utf-8')
        failed = subprocess.run([sys.executable, '-m', 'unittest', 'test_acceptance', '-v'], cwd=project,
                                capture_output=True, text=True, encoding='utf-8', timeout=30)
        assert failed.returncode != 0, 'New requirement unexpectedly already passes'
        report['observed_failure'] = failed.stderr[-2500:]
        resumed = run_stage('Continue this same project. I added test_acceptance.py for list --contains TEXT: '
                            'filter notes case-insensitively and combine correctly with --upper without writing storage. '
                            'Run that test first to observe failure. Implement the feature using the patch tool, update '
                            'README, and run all tests. Do not modify test_acceptance.py or weaken the existing tests.', session)
        assert resumed == session, 'Codex did not resume the same thread'
        assert (project/'test_acceptance.py').read_text(encoding='utf-8') == checks.REGRESSION
        assert all((project/name).read_bytes() == content for name, content in original_tests.items()), 'Existing tests changed during resume'
        result = subprocess.run([sys.executable, '-m', 'unittest', 'discover', '-v'], cwd=project,
                                capture_output=True, text=True, encoding='utf-8', timeout=60)
        report['independent_tests'] = {'exit': result.returncode, 'output': result.stderr[-6000:]}
        assert result.returncode == 0, report['independent_tests']
        count = re.search(r'Ran (\d+) tests?', result.stderr)
        assert count and int(count.group(1)) >= 7, report['independent_tests']
        report['independent_cli_checks'] = checks.verify_cli(project)
        if args.restart_cloud_before_result:
            assert report.get('forced_cloud_restart'), 'Cloud recovery was not exercised'
        # These exact messages originate from transport failures. They count as
        # recovered only after both real client turns and independent checks pass.
        # Validation errors, upstream model failures and HTTP 400 remain failures.
        recoverable = ('Quick cloud WebSocket disconnected', 'Quick cloud connection closed',
                       'Quick cloud registration request failed', 'Quick cloud registration response disconnected')
        assert all(e.get('type') == 'response.failed' and e.get('response', {}).get('error', {}).get('code') == 'server_error'
                   and e.get('response', {}).get('error', {}).get('message') in recoverable
                   for e in report['errors']), report['errors']
        report['recovered_transport_errors'] = [e['response']['error']['message'] for e in report['errors']]
        assert any(c['type'] == 'custom_tool_call' and c['name'] in ('apply_patch', 'exec') for c in report['tool_calls']), report['tool_calls']
        assert any(e.get('type') == 'item.completed' and e.get('item', {}).get('type') == 'file_change'
                   for stage in report['stages'] for e in stage['events']), 'Codex did not apply file changes'
        report['passed'] = True
        print(json.dumps({'passed': True, 'project': str(project), 'thread': session,
                          'calls': len(report['tool_calls']), 'tests': int(count.group(1))}), flush=True)
    finally:
        proxy.shutdown()
        (root/'quickway-acceptance.json').write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')


if __name__ == '__main__': main()
