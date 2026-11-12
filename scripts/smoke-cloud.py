"""Opt-in synthetic history, multi-tool and cancellation checks (uses quota)."""
import argparse
import importlib.util
import json
import pathlib
import urllib.request
import uuid

spec = importlib.util.spec_from_file_location('messages_smoke', pathlib.Path(__file__).with_name('smoke-test.py'))
smoke = importlib.util.module_from_spec(spec)
spec.loader.exec_module(smoke)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--base', required=True)
    parser.add_argument('--key-file', required=True)
    parser.add_argument('--model', default='quick-smart')
    args = parser.parse_args()
    key = pathlib.Path(args.key_file).read_text().strip()
    marker = 'HISTORY_' + uuid.uuid4().hex[:12]

    def send(messages, **options):
        return smoke.post(args.base, key, dict(model=args.model, max_tokens=1024, messages=messages, **options))

    def answer(result):
        assert result['stop_reason'] == 'end_turn', result
        return ''.join(b.get('text', '') for b in result['content'])

    messages = [{'role': 'user', 'content': 'Remember this code: ' + marker},
                {'role': 'assistant', 'content': 'I have saved the code.'},
                {'role': 'user', 'content': 'Reply only with the code I asked you to remember. Do not use tools.'}]
    result = send(messages, system='Follow the user instruction exactly.')
    assert marker in answer(result), result
    messages += [{'role': 'assistant', 'content': result['content']},
                 {'role': 'user', 'content': 'Repeat that same code once more, without tools.'}]
    result = send(messages, system=[{'type': 'text', 'text': 'Follow the user instruction exactly.', 'cache_control': {'type': 'ephemeral'}}])
    assert marker in answer(result), result
    print(json.dumps({'cold_text_history': True, 'immediate_followup': True}), flush=True)

    tool_marker = 'TOOL_HISTORY_' + uuid.uuid4().hex[:12]
    messages = [{'role': 'user', 'content': 'Call echo and remember the returned code.'},
                {'role': 'assistant', 'content': [{'type': 'tool_use', 'id': 'synthetic_call_1', 'name': 'echo', 'input': {'text': 'sample'}}]},
                {'role': 'user', 'content': [{'type': 'tool_result', 'tool_use_id': 'synthetic_call_1', 'content': [{'type': 'text', 'text': tool_marker}]}]},
                {'role': 'assistant', 'content': 'I have saved the returned code.'},
                {'role': 'user', 'content': 'What code did echo return? Reply only with the code, without tools.'}]
    assert tool_marker in answer(send(messages))
    print(json.dumps({'cold_completed_tool_history': True}), flush=True)

    tools = [{'name': 'echo', 'description': 'Return the given synthetic text.',
              'input_schema': {'type': 'object', 'properties': {'text': {'type': 'string'}}, 'required': ['text']}}]
    messages = [{'role': 'user', 'content': 'Call the echo tool twice: once with text FIRST, once with text SECOND. You may call them in parallel. Then quote both exact returned values. Use only the echo tool on the QuickWay API session device.'}]
    calls = []
    for _ in range(8):
        result = send(messages, tools=tools)
        messages.append({'role': 'assistant', 'content': result['content']})
        if result['stop_reason'] == 'end_turn':
            text = answer(result)
            assert 'RESULT_FIRST' in text and 'RESULT_SECOND' in text, text
            break
        assert result['stop_reason'] == 'tool_use', result
        replies = []
        for block in result['content']:
            if block['type'] != 'tool_use':
                continue
            assert block['name'] == 'echo' and block['input'].get('text') in ('FIRST', 'SECOND'), {'name':block['name'],'input_keys':list(block['input'])}
            calls.append(block['input']['text'])
            replies.append({'type': 'tool_result', 'tool_use_id': block['id'], 'content': 'RESULT_' + block['input']['text']})
        assert replies, result
        messages.append({'role': 'user', 'content': replies})
    else:
        raise AssertionError('Multi-tool conversation did not finish')
    assert sorted(calls) == ['FIRST', 'SECOND'], calls
    messages.append({'role': 'user', 'content': 'Repeat only the two returned RESULT_ values, without calling tools again.'})
    text = answer(send(messages, tools=tools))
    assert 'RESULT_FIRST' in text and 'RESULT_SECOND' in text, text
    print(json.dumps({'multiple_tool_calls': True, 'post_tool_followup': True}), flush=True)

    body = {'model': args.model, 'max_tokens': 4096, 'stream': True,
            'messages': [{'role': 'user', 'content': 'Print the integers from 1 to 5000, one per line, without tools.'}]}
    request = urllib.request.Request(args.base + '/v1/messages', data=json.dumps(body).encode(),
                                     headers={'x-api-key': key, 'Content-Type': 'application/json'})
    with urllib.request.urlopen(request, timeout=120) as response:
        for line in response:
            if line.startswith(b'data: '):
                event = json.loads(line[6:])
                assert event.get('type') != 'error', event
                if event.get('type') == 'content_block_delta':
                    break
        else:
            raise AssertionError('No stream content before cancellation')
    assert 'CANCEL_OK' in answer(send([{'role': 'user', 'content': 'Reply exactly CANCEL_OK, without tools.'}]))
    print(json.dumps({'cancel_then_request': True}), flush=True)


if __name__ == '__main__':
    main()
