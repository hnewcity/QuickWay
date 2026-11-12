"""Opt-in real CC project acceptance, including a mid-tool user update.

Uses the full normal Claude Code tool catalog and settings. Only synthetic
files are used; the resulting project and a credential-free report are kept.
"""
import argparse
import http.server
import json
import os
import pathlib
import queue
import re
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.request


REGRESSION = r'''import json, pathlib, subprocess, sys, tempfile, unittest
SCRIPT = str(pathlib.Path(__file__).with_name("notes.py"))
class Acceptance(unittest.TestCase):
    def test_filter_is_case_insensitive_and_composes_with_upper(self):
        with tempfile.TemporaryDirectory() as d:
            file = pathlib.Path(d)/"notes.json"
            file.write_text(json.dumps(["Alpha", "beta", "alphabet", "中文"]), encoding="utf-8")
            before = file.read_bytes()
            r = subprocess.run([sys.executable, SCRIPT, "--file", str(file), "list", "--contains", "ALP", "--upper"], capture_output=True, text=True, encoding="utf-8")
            self.assertEqual(r.returncode, 0, r.stderr)
            self.assertEqual(json.loads(r.stdout), ["ALPHA", "ALPHABET"])
            self.assertEqual(file.read_bytes(), before)
if __name__ == "__main__": unittest.main()
'''


def verify_cli(directory):
    """Check behavior independently of any tests authored by CC."""
    with tempfile.TemporaryDirectory(prefix='quickway-notes-check-') as fixture:
        storage = pathlib.Path(fixture, 'notes.json')
        def run(*arguments, code=0):
            result = subprocess.run([sys.executable, str(directory/'notes.py'), '--file', str(storage), *arguments],
                                    capture_output=True, text=True, encoding='utf-8', timeout=15)
            assert result.returncode == code, (arguments, result.returncode, result.stderr)
            assert 'Traceback' not in result.stderr, result.stderr
            return result.stdout
        assert json.loads(run('list')) == []
        run('add', 'Caf\u00e9 \u4e2d\u6587')
        run('add', 'Beta')
        before = storage.read_bytes()
        assert json.loads(run('list')) == ['Caf\u00e9 \u4e2d\u6587', 'Beta']
        assert json.loads(run('list', '--upper')) == ['CAF\u00c9 \u4e2d\u6587', 'BETA']
        assert json.loads(run('list', '--contains', 'CAF\u00c9', '--upper')) == ['CAF\u00c9 \u4e2d\u6587']
        run('add', '', code=2)
        assert storage.read_bytes() == before
        for invalid in ('{broken', '{}'):
            storage.write_text(invalid, encoding='utf-8')
            run('list', code=2)
            run('add', 'Do not overwrite', code=2)
            assert storage.read_text(encoding='utf-8') == invalid
    return True


def main():
    # Propagate UTF-8 to test subprocesses and their nested CLI invocations on
    # Windows, including failing unittest tracebacks with non-ASCII paths.
    os.environ['PYTHONUTF8'] = '1'
    os.environ['PYTHONIOENCODING'] = 'utf-8'
    parser = argparse.ArgumentParser()
    parser.add_argument('--base', required=True)
    parser.add_argument('--key-file', required=True)
    parser.add_argument('--model', default='quick-smart')
    parser.add_argument('--edit-probe', action='store_true', help=argparse.SUPPRESS)
    args = parser.parse_args()
    key = pathlib.Path(args.key_file).read_text().strip()
    directory = pathlib.Path(tempfile.mkdtemp(prefix='quickway-cc-project-'))
    report = {'directory': str(directory), 'requests': [], 'calls': [], 'results': [], 'errors': []}
    emitted_tools = {}
    observed_replay_changes = set()
    print(json.dumps({'project': str(directory)}), flush=True)

    class Proxy(http.server.BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_GET(self):
            self.forward(None)

        def do_POST(self):
            self.forward(self.rfile.read(int(self.headers.get('Content-Length', 0))))

        def forward(self, body):
            messages_request = self.path.startswith('/v1/messages') and 'count_tokens' not in self.path
            if messages_request and body:
                data = json.loads(body)
                for message in data.get('messages', []):
                    if message.get('role') != 'assistant' or not isinstance(message.get('content'), list):
                        continue
                    for block in message['content']:
                        original = emitted_tools.get(block.get('id'))
                        if original and original != block.get('input') and block.get('id') not in observed_replay_changes:
                            observed_replay_changes.add(block['id'])
                            # These inputs belong exclusively to this synthetic
                            # fixture; never dump system prompts or credentials.
                            changed = {k: {'sent': original.get(k), 'replayed': block.get('input', {}).get(k)}
                                       for k in set(original) | set(block.get('input', {}))
                                       if original.get(k) != block.get('input', {}).get(k)}
                            detail = {'tool': block.get('name'), 'changed': changed}
                            report.setdefault('tool_replay_changes', []).append(detail)
                            print(json.dumps({'tool_replay_change': detail}), flush=True)
                # Interactive CC embeds mid-turn text in tool-result content;
                # stream-json CC can send adjacent user messages. Normalization
                # folds both into the same turn, preserving their ordering.
                types = []
                for message in reversed(data.get('messages', [])):
                    if message.get('role') == 'assistant':
                        break
                    content = message.get('content', [])
                    blocks = content if isinstance(content, list) else [{'type': 'text', 'text': content}]
                    for block in blocks:
                        # SDK input can be surfaced as a system reminder too.
                        # Ignore only the numeric budget bookkeeping here.
                        if message.get('role') == 'system' and block.get('type') == 'text' and block.get('text', '').strip().startswith('<total_tokens>'):
                            continue
                        types.append(block.get('type'))
                shape = {'tools': len(data.get('tools', [])), 'messages': len(data.get('messages', [])),
                         'mixed_tool_user_text': 'tool_result' in types and 'text' in types,
                         'trailing_roles': [m.get('role') for m in data.get('messages', [])[-3:]],
                         'update_in_request': 'Additional requirement while the shell is running:' in json.dumps(data.get('messages', []))}
                report['requests'].append(shape)
                print(json.dumps({'request': shape}), flush=True)
            headers = {k: v for k, v in self.headers.items() if k.lower() not in
                       ('host', 'content-length', 'connection', 'authorization', 'x-api-key')}
            headers['Authorization'] = 'Bearer ' + key
            request = urllib.request.Request(args.base + self.path, data=body, headers=headers)
            try:
                response = urllib.request.urlopen(request, timeout=190)
            except urllib.error.HTTPError as error:
                response = error
            self.send_response(response.status)
            self.send_header('Content-Type', response.headers.get('Content-Type', 'application/json'))
            self.send_header('Connection', 'close')
            self.end_headers()
            pending = b''
            tool_blocks = {}
            try:
                with response:
                    while chunk := response.read1(65536):
                        self.wfile.write(chunk)
                        self.wfile.flush()
                        pending += chunk
                        while b'\n' in pending:
                            line, pending = pending.split(b'\n', 1)
                            if line.startswith(b'data: '):
                                event = json.loads(line[6:])
                                if event.get('type') == 'content_block_start' and event.get('content_block', {}).get('type') == 'tool_use':
                                    tool_blocks[event['index']] = {'id': event['content_block']['id'], 'json': ''}
                                if event.get('type') == 'content_block_delta' and event.get('delta', {}).get('type') == 'input_json_delta':
                                    tool_blocks[event['index']]['json'] += event['delta']['partial_json']
                                if event.get('type') == 'content_block_stop' and event['index'] in tool_blocks:
                                    block = tool_blocks[event['index']]
                                    emitted_tools[block['id']] = json.loads(block['json'])
                                if event.get('type') == 'error':
                                    report['errors'].append(event['error'])
                                    print(json.dumps({'api_error': event['error']}), flush=True)
                                if event.get('type') == 'message_delta':
                                    report.setdefault('models', []).append(event.get('quickway', {}).get('served_model'))
                    if response.status >= 400:
                        report['errors'].append({'status': response.status, 'body': pending.decode('utf-8')[:1200]})
            except (BrokenPipeError, ConnectionResetError):
                pass

    proxy = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Proxy)
    threading.Thread(target=proxy.serve_forever, daemon=True).start()
    env = os.environ.copy()
    env.update(ANTHROPIC_BASE_URL=f'http://127.0.0.1:{proxy.server_port}', ANTHROPIC_AUTH_TOKEN=key,
               ANTHROPIC_MODEL=args.model, CLAUDE_CODE_MAX_CONTEXT_TOKENS='200000',
               ANTHROPIC_CUSTOM_MODEL_OPTION=args.model, CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC='1')
    for name in ('ANTHROPIC_API_KEY', 'CLAUDE_CODE_USE_BEDROCK', 'CLAUDE_CODE_USE_VERTEX', 'CLAUDE_CODE_USE_FOUNDRY'):
        env.pop(name, None)
    settings = {'env': {k: v for k, v in env.items() if k.startswith(('ANTHROPIC_', 'CLAUDE_CODE_'))}}
    command = ['claude', '--settings', json.dumps(settings), '--no-session-persistence',
               '--allowedTools', 'Read,Write,Edit,Glob,Grep,PowerShell,Bash', '--permission-prompts', 'none',
               '--model', args.model, '--effort', 'low', '--input-format', 'stream-json',
               '--output-format', 'stream-json', '--verbose', '-p']
    child = subprocess.Popen(command, cwd=directory, env=env, stdin=subprocess.PIPE,
                             stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding='utf-8')
    lines = queue.Queue()
    def read_lines(stream):
        for line in stream:
            lines.put(line)
    threading.Thread(target=read_lines, args=(child.stdout,), daemon=True).start()
    threading.Thread(target=read_lines, args=(child.stderr,), daemon=True).start()

    def send(text):
        child.stdin.write(json.dumps({'type': 'user', 'message': {'role': 'user', 'content': text}}) + '\n')
        child.stdin.flush()

    if args.edit_probe:
        (directory/'fixture.txt').write_text('FIRST\nSECOND\n', encoding='utf-8')
        send('Use the Edit tool to replace FIRST with CHANGED in fixture.txt in the current directory. Then use Read to verify it. Do not use Bash or PowerShell. Reply EDIT_OK after verification.')
    else:
        send('Build a complete small Python standard-library project in the current empty directory. '
         'First run a shell command that waits 6 seconds and prints PROJECT_START; then create notes.py, README.md, '
         'and unittest tests. Do not ask questions. Work only in this directory; no network, upload, git, or external files. '
         'CLI: python notes.py --file PATH add TEXT appends a nonempty note to a UTF-8 JSON array file; '
         'python notes.py --file PATH list prints that array as JSON. Missing storage means an empty array. '
         'Empty notes and corrupt/non-array storage must exit 2 with a useful stderr and no traceback, '
         'preserving the original file. Support Unicode. Write at least six meaningful tests and run them. '
         'Actually create the files with client tools, execute tests, and fix failures until they pass. '
         'Finish with a brief plain-text summary of the files and test result.')
    steered = False
    stage = 1
    deadline = time.monotonic() + 900
    try:
        while time.monotonic() < deadline:
            try:
                line = lines.get(timeout=1)
            except queue.Empty:
                if child.poll() is not None:
                    raise AssertionError(f'CC exited early: {child.returncode}')
                continue
            if not line.startswith('{'):
                continue
            event = json.loads(line)
            if event.get('type') == 'assistant':
                for block in event.get('message', {}).get('content', []):
                    if block.get('type') == 'text':
                        assert '<decision question=' not in block.get('text', '') and '<option description=' not in block.get('text', ''), 'Native widget leaked'
                    if block.get('type') == 'tool_use':
                        call = {'stage': stage, 'name': block.get('name')}
                        report['calls'].append(call)
                        print(json.dumps({'tool': call}), flush=True)
                        if not steered and block.get('name') in ('PowerShell', 'Bash'):
                            time.sleep(1)
                            send('Additional requirement while the shell is running: list must accept --upper, '
                                 'return uppercased strings in its JSON output, and leave stored data unchanged. '
                                 'Include a regression test for this. Continue building the same project.')
                            steered = True
                            print(json.dumps({'mid_tool_user_message_sent': True}), flush=True)
            if event.get('type') == 'result':
                report['results'].append({k: event.get(k) for k in ('subtype', 'is_error', 'num_turns', 'result', 'permission_denials')})
                assert not event.get('is_error'), report['results'][-1]
                assert not event.get('permission_denials'), report['results'][-1]
                if args.edit_probe:
                    assert (directory/'fixture.txt').read_text(encoding='utf-8') == 'CHANGED\nSECOND\n'
                    assert 'EDIT_OK' in event.get('result', '')
                    assert not report['errors'], report['errors']
                    report['passed'] = True
                    return
                assert (directory/'notes.py').exists(), 'CC did not create the project'
                print(json.dumps({'stage_completed': stage, 'turns': event.get('num_turns')}), flush=True)
                if stage == 1:
                    assert steered and any(r['mixed_tool_user_text'] for r in report['requests']), 'Did not exercise mid-tool input'
                    (directory/'test_acceptance.py').write_text(REGRESSION, encoding='utf-8')
                    failed = subprocess.run([sys.executable, '-m', 'unittest', 'test_acceptance', '-v'], cwd=directory, capture_output=True, text=True, encoding='utf-8')
                    assert failed.returncode != 0, 'New requirement should first fail'
                    report['observed_regression_failure'] = failed.stderr[-2500:]
                    stage = 2
                    send('I added test_acceptance.py for a new requirement: list --contains TEXT filters notes '
                         'case-insensitively and composes with --upper, without modifying storage. Run this test '
                         'first to observe the failure. Then implement the feature, update README, and run all '
                         'tests until passing. Do not modify test_acceptance.py or weaken any existing tests.')
                else:
                    break
        else:
            raise AssertionError('Project acceptance timed out')
        assert (directory/'test_acceptance.py').read_text(encoding='utf-8') == REGRESSION, 'Acceptance test changed'
        result = subprocess.run([sys.executable, '-m', 'unittest', 'discover', '-v'], cwd=directory, capture_output=True, text=True, encoding='utf-8', timeout=60)
        report['independent_tests'] = {'exit': result.returncode, 'output': result.stderr[-6000:]}
        assert result.returncode == 0, report['independent_tests']
        count = re.search(r'Ran (\d+) tests?', result.stderr)
        assert count and int(count.group(1)) >= 7, report['independent_tests']
        report['independent_cli_checks'] = verify_cli(directory)
        assert not report['errors'], report['errors']
        assert max(r['tools'] for r in report['requests']) >= 15, 'Tool catalog was reduced'
        report['passed'] = True
        print(json.dumps({'passed': True, 'project': str(directory), 'tool_calls': len(report['calls']), 'tests': result.stderr[-2500:]}), flush=True)
    finally:
        if child.poll() is None:
            child.stdin.close()
            try:
                child.wait(timeout=15)
            except subprocess.TimeoutExpired:
                child.terminate()
                child.wait(timeout=10)
        proxy.shutdown()
        (directory/'quickway-acceptance.json').write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')


if __name__ == '__main__':
    main()
