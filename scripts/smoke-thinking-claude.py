"""Opt-in Claude Code thinking/tool checks. Only reads a synthetic local fixture."""
import argparse
import http.server
import json
import os
import pathlib
import shutil
import subprocess
import tempfile
import threading
import urllib.error
import urllib.request

p = argparse.ArgumentParser()
p.add_argument('--base', default='http://127.0.0.1:8742')
p.add_argument('--key-file', required=True)
p.add_argument('--model', default='claude-opus-4-8')
p.add_argument('--effort', choices=['off', 'low', 'medium', 'high'], required=True)
a = p.parse_args()
key = pathlib.Path(a.key_file).read_text(encoding='utf-8').strip()
requests = []
errors = []


class Recorder(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        body = self.rfile.read(int(self.headers.get('Content-Length', 0)))
        profile = self.headers.get('x-quickway-thinking-effort')
        requests.append(profile)
        headers = {'Content-Type': 'application/json', 'x-api-key': key}
        if profile:
            headers['x-quickway-thinking-effort'] = profile
        try:
            upstream = urllib.request.urlopen(urllib.request.Request(a.base + self.path, data=body, headers=headers), timeout=150)
        except urllib.error.HTTPError as error:
            upstream = error
        self.send_response(upstream.status)
        self.send_header('Content-Type', upstream.headers.get('Content-Type', 'application/json'))
        self.end_headers()
        if upstream.status != 200:
            errors.append(upstream.status)
        with upstream:
            for line in upstream:
                if line.startswith(b'data: '):
                    event = json.loads(line[6:])
                    if event.get('type') == 'error':
                        errors.append(event.get('error', {}).get('type', 'stream_error'))
                self.wfile.write(line)
                self.wfile.flush()


server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Recorder)
threading.Thread(target=server.serve_forever, daemon=True).start()
try:
    with tempfile.TemporaryDirectory(prefix='quickway-thinking-') as directory:
        directory = pathlib.Path(directory)
        fixture = directory / 'fixture.txt'
        fixture.write_text('QUICKWAY_THINKING_TOOL_OK\n', encoding='utf-8')
        enabled = a.effort != 'off'
        env_options = {
            'ANTHROPIC_BASE_URL': f'http://127.0.0.1:{server.server_port}',
            'ANTHROPIC_AUTH_TOKEN': key,
            'ANTHROPIC_API_KEY': key,
            'ANTHROPIC_CUSTOM_HEADERS': f'x-quickway-thinking-effort: {a.effort}',
            'CLAUDE_CODE_EFFORT_LEVEL': a.effort if enabled else 'low',
            'MAX_THINKING_TOKENS': str({'off': 0, 'low': 1024, 'medium': 4096, 'high': 8192}[a.effort]),
            'CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS': '1',
            'ENABLE_TOOL_SEARCH': 'false',
        }
        settings = directory / 'settings.json'
        settings.write_text(json.dumps({'alwaysThinkingEnabled': enabled, 'effortLevel': a.effort if enabled else 'low', 'env': env_options}), encoding='utf-8')
        environment = os.environ.copy()
        environment.update(env_options)
        prompt = f'Use Read to read {fixture.as_posix()}. Reply exactly with its contents. Read it first; do not guess.'
        result = subprocess.run([shutil.which('claude'), '--bare', '--settings', str(settings), '-p', prompt,
                                 '--model', a.model, '--tools', 'Read', '--allowedTools', 'Read', '--max-turns', '3',
                                 '--no-session-persistence', '--output-format', 'stream-json', '--verbose'],
                                env=environment, cwd=directory, capture_output=True, text=True, encoding='utf-8', timeout=180)
        events = [json.loads(line) for line in result.stdout.splitlines() if line.startswith('{')]
        reads = [block for event in events if event.get('type') == 'assistant'
                 for block in event.get('message', {}).get('content', []) if block.get('type') == 'tool_use' and block.get('name') == 'Read']
        final = next((event for event in reversed(events) if event.get('type') == 'result'), {})
        ok = result.returncode == 0 and bool(reads) and not final.get('is_error') and 'QUICKWAY_THINKING_TOOL_OK' in final.get('result', '') and not errors and len(requests) >= 2 and all(profile == a.effort for profile in requests)
        print(json.dumps({'model': a.model, 'effort': a.effort, 'ok': ok, 'read_calls': len(reads), 'request_profiles': requests, 'errors': errors}), flush=True)
        if not ok:
            raise SystemExit(1)
finally:
    server.shutdown()
    server.server_close()
