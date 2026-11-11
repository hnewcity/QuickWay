"""Opt-in live checks for the OpenAI-compatible endpoints. Spends a few Quick tokens.

Requires the official SDK: pip install openai
"""
import argparse, json, pathlib
from openai import OpenAI

TOOLS_CHAT = [{'type': 'function', 'function': {'name': 'echo', 'description': 'Return the supplied text.',
    'parameters': {'type': 'object', 'properties': {'text': {'type': 'string'}}, 'required': ['text']}}}]
TOOLS_RESP = [{'type': 'function', 'name': 'echo', 'description': 'Return the supplied text.',
    'parameters': {'type': 'object', 'properties': {'text': {'type': 'string'}}, 'required': ['text']}}]
ASK = 'Call the echo tool with text hello. Then quote the exact tool response verbatim, even if it differs from the requested text.'


def chat(client, model, effort):
    extra = {'reasoning_effort': effort} if effort else {}
    r = client.chat.completions.create(model=model, max_tokens=4096, messages=[{'role': 'user', 'content': 'Reply exactly QUICKWAY_OK.'}], **extra)
    assert r.choices[0].finish_reason == 'stop' and 'QUICKWAY_OK' in r.choices[0].message.content, r
    text, usage = '', None
    for chunk in client.chat.completions.create(model=model, max_tokens=4096, stream=True, stream_options={'include_usage': True},
                                                messages=[{'role': 'user', 'content': 'Reply exactly QUICKWAY_OK.'}], **extra):
        if chunk.choices and chunk.choices[0].delta.content: text += chunk.choices[0].delta.content
        if chunk.usage: usage = chunk.usage
    assert 'QUICKWAY_OK' in text and usage and usage.total_tokens > 0, (text, usage)
    for stream in (False, True):
        messages = [{'role': 'user', 'content': ASK}]
        call = None
        if stream:
            calls = {}
            for chunk in client.chat.completions.create(model=model, max_tokens=4096, messages=messages, tools=TOOLS_CHAT, stream=True, **extra):
                for t in (chunk.choices[0].delta.tool_calls or []) if chunk.choices else []:
                    c = calls.setdefault(t.index, {'id': '', 'name': '', 'arguments': ''})
                    c['id'] += t.id or ''; c['name'] += (t.function.name or '') if t.function else ''
                    c['arguments'] += (t.function.arguments or '') if t.function else ''
            call = calls[0]
        else:
            r = client.chat.completions.create(model=model, max_tokens=4096, messages=messages, tools=TOOLS_CHAT, **extra)
            assert r.choices[0].finish_reason == 'tool_calls', r
            t = r.choices[0].message.tool_calls[0]
            call = {'id': t.id, 'name': t.function.name, 'arguments': t.function.arguments}
        assert call['name'] == 'echo' and isinstance(json.loads(call['arguments']), dict), call
        messages += [{'role': 'assistant', 'content': None, 'tool_calls': [{'id': call['id'], 'type': 'function',
                      'function': {'name': call['name'], 'arguments': call['arguments']}}]},
                     {'role': 'tool', 'tool_call_id': call['id'], 'content': 'ECHO_ROUNDTRIP_OK'}]
        r = client.chat.completions.create(model=model, max_tokens=4096, messages=messages, tools=TOOLS_CHAT, **extra)
        assert 'ECHO_ROUNDTRIP_OK' in (r.choices[0].message.content or ''), r


def responses(client, model, effort):
    extra = {'reasoning': {'effort': effort}} if effort else {}
    r = client.responses.create(model=model, max_output_tokens=4096, input='Reply exactly QUICKWAY_OK.', **extra)
    assert r.status == 'completed' and 'QUICKWAY_OK' in r.output_text, r
    with client.responses.stream(model=model, max_output_tokens=4096, input='Reply exactly QUICKWAY_OK.', **extra) as s:
        final = s.get_final_response()
    assert final.status == 'completed' and 'QUICKWAY_OK' in final.output_text, final
    for stream in (False, True):
        items = [{'role': 'user', 'content': ASK}]
        if stream:
            with client.responses.stream(model=model, max_output_tokens=4096, input=items, tools=TOOLS_RESP, **extra) as s:
                r = s.get_final_response()
        else:
            r = client.responses.create(model=model, max_output_tokens=4096, input=items, tools=TOOLS_RESP, **extra)
        call = next(o for o in r.output if o.type == 'function_call')
        assert call.name == 'echo' and isinstance(json.loads(call.arguments), dict), call
        # Replay the whole output (reasoning included) statelessly.
        items += [o.model_dump(exclude_none=True) for o in r.output]
        items.append({'type': 'function_call_output', 'call_id': call.call_id, 'output': 'ECHO_ROUNDTRIP_OK'})
        r = client.responses.create(model=model, max_output_tokens=4096, input=items, tools=TOOLS_RESP, **extra)
        assert 'ECHO_ROUNDTRIP_OK' in r.output_text, r


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--base', default='http://127.0.0.1:8741')
    p.add_argument('--key-file', default='.quickway/proxy.key')
    p.add_argument('--model', default='claude-sonnet-4-6')
    p.add_argument('--effort', choices=['none', 'low', 'medium', 'high'])
    a = p.parse_args()
    client = OpenAI(base_url=a.base + '/v1', api_key=pathlib.Path(a.key_file).read_text().strip(), max_retries=0, timeout=180)
    chat(client, a.model, a.effort)
    responses(client, a.model, a.effort)
    print(json.dumps({'model': a.model, 'effort': a.effort, 'chat': True, 'responses': True}))


if __name__ == '__main__':
    main()
