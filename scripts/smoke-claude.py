"""Opt-in real Claude Code test; execution is limited to a synthetic Read."""
import argparse
import json
import os
import pathlib
import subprocess
import tempfile
import uuid


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--base', required=True)
    parser.add_argument('--key-file', required=True)
    parser.add_argument('--model', default='quick-smart')
    parser.add_argument('--normal', action='store_true', help='Use normal settings and the full default tool catalog')
    args = parser.parse_args()
    env = os.environ.copy()
    env.update(ANTHROPIC_BASE_URL=args.base,
               ANTHROPIC_AUTH_TOKEN=pathlib.Path(args.key_file).read_text().strip(),
               ANTHROPIC_MODEL=args.model, CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC='1')
    if args.model.startswith('quick-'):
        env['CLAUDE_CODE_MAX_CONTEXT_TOKENS'] = '200000'
        env['ANTHROPIC_CUSTOM_MODEL_OPTION'] = args.model
    for name in ('ANTHROPIC_API_KEY', 'CLAUDE_CODE_USE_BEDROCK', 'CLAUDE_CODE_USE_VERTEX', 'CLAUDE_CODE_USE_FOUNDRY'):
        env.pop(name, None)
    marker = 'CLAUDE_READ_' + uuid.uuid4().hex[:12]
    with tempfile.TemporaryDirectory(prefix='quickway-claude-fixture-') as directory:
        path = pathlib.Path(directory, 'fixture.txt')
        path.write_text(marker, encoding='utf-8')
        isolation = [] if args.normal else ['--bare', '--restricted', '--strict-mcp-config', '--tools', 'Read']
        # Explicit settings override a user's provider env without altering their file.
        settings = {'env': {key: value for key, value in env.items() if key.startswith(('ANTHROPIC_', 'CLAUDE_CODE_'))}}
        result = subprocess.run(['claude', *isolation, '--settings', json.dumps(settings),
                                 '--no-session-persistence', '--allowedTools', 'Read',
                                 '--permission-prompts', 'none', '--model', args.model, '--effort', 'low',
                                 '--output-format', 'stream-json', '--verbose', '-p',
                                 'Use the Read tool to read fixture.txt in the current directory. Reply with only the exact code from that file.'],
                                cwd=directory, env=env, capture_output=True, text=True, encoding='utf-8', timeout=180)
    events = [json.loads(line) for line in result.stdout.splitlines() if line.startswith('{')]
    calls = [block for event in events if event.get('type') == 'assistant'
             for block in event.get('message', {}).get('content', []) if block.get('type') == 'tool_use']
    finals = [event for event in events if event.get('type') == 'result']
    assert result.returncode == 0 and finals and not finals[-1].get('is_error'), {
        'exit': result.returncode, 'result': finals[-1] if finals else result.stderr[-1000:]}
    assert any(call.get('name') == 'Read' for call in calls), {'call_names': [c.get('name') for c in calls], 'result': finals[-1].get('result', '')}
    assert marker in finals[-1].get('result', ''), {'call_names': [c.get('name') for c in calls], 'input_keys': [list(c.get('input', {})) for c in calls], 'result': finals[-1].get('result', '')}
    print(json.dumps({'claude_code_read_tool': True, 'exact_fixture_result': True, 'normal_configuration': args.normal}))


if __name__ == '__main__':
    main()
