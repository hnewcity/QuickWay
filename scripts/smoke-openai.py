"""Opt-in live checks for the OpenAI-compatible endpoints. Spends a few Quick tokens.

Requires the official SDK: pip install openai
"""
import argparse, json, pathlib
from concurrent.futures import ThreadPoolExecutor
from uuid import uuid4
from openai import OpenAI

TOOLS_CHAT = [{'type': 'function', 'function': {'name': 'echo', 'description': 'Return the supplied text.',
    'parameters': {'type': 'object', 'properties': {'text': {'type': 'string'}}, 'required': ['text']}}}]
TOOLS_RESP = [{'type': 'function', 'name': 'echo', 'description': 'Return the supplied text.',
    'parameters': {'type': 'object', 'properties': {'text': {'type': 'string'}}, 'required': ['text']}}]
ASK = 'Call the echo tool with text hello. Then quote the exact tool response verbatim, even if it differs from the requested text.'


def missing_callback(p, text):
    # Keep model output and device identifiers out of the diagnostic.
    msg = (text or '').lower()
    category = ('device reported disconnected' if any(x in msg for x in ('not connected', 'disconnected', 'offline'))
                else 'tool reported unavailable' if any(x in msg for x in ('not available', 'unavailable', 'not found'))
                else 'ordinary text or no tool')
    return f'{p}: expected one tool call ({category})'


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
        assistant_text = ''
        if stream:
            calls = {}
            for chunk in client.chat.completions.create(model=model, max_tokens=4096, messages=messages, tools=TOOLS_CHAT, stream=True, **extra):
                if chunk.choices and chunk.choices[0].delta.content:
                    assistant_text += chunk.choices[0].delta.content
                for t in (chunk.choices[0].delta.tool_calls or []) if chunk.choices else []:
                    c = calls.setdefault(t.index, {'id': '', 'name': '', 'arguments': ''})
                    c['id'] += t.id or ''; c['name'] += (t.function.name or '') if t.function else ''
                    c['arguments'] += (t.function.arguments or '') if t.function else ''
            assert set(calls) == {0}, missing_callback('chat (stream)', assistant_text)
            call = calls[0]
        else:
            r = client.chat.completions.create(model=model, max_tokens=4096, messages=messages, tools=TOOLS_CHAT, **extra)
            calls = r.choices[0].message.tool_calls or []
            assert r.choices[0].finish_reason == 'tool_calls' and len(calls) == 1, missing_callback('chat', r.choices[0].message.content)
            t = calls[0]
            assistant_text = r.choices[0].message.content or ''
            call = {'id': t.id, 'name': t.function.name, 'arguments': t.function.arguments}
        assert call['name'] == 'echo' and json.loads(call['arguments']) == {'text': 'hello'}, {'name':call['name'],'input_keys':list(json.loads(call['arguments']))}
        messages += [{'role': 'assistant', 'content': assistant_text or None, 'tool_calls': [{'id': call['id'], 'type': 'function',
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
        calls = [o for o in r.output if o.type == 'function_call']
        assert r.status == 'completed' and len(calls) == 1, missing_callback('responses', r.output_text)
        call = calls[0]
        assert call.name == 'echo' and json.loads(call.arguments) == {'text': 'hello'}, {'name':call.name,'input_keys':list(json.loads(call.arguments))}
        # Replay the whole output (reasoning included) statelessly.
        items += [o.model_dump(exclude_none=True) for o in r.output]
        items.append({'type': 'function_call_output', 'call_id': call.call_id, 'output': 'ECHO_ROUNDTRIP_OK'})
        r = client.responses.create(model=model, max_output_tokens=4096, input=items, tools=TOOLS_RESP, **extra)
        assert 'ECHO_ROUNDTRIP_OK' in r.output_text, r


def parallel_tools(client, model, effort):
    """Hold two calls to the same named tool open; each client must receive only its own result."""
    protocols = ('chat', 'responses')
    inputs = {p: uuid4().hex for p in protocols}
    results = {p: uuid4().hex for p in protocols}
    chat_opts = {'reasoning_effort': effort} if effort else {}
    resp_opts = {'reasoning': {'effort': effort}} if effort else {}

    def begin(p):
        prompt = f'Call the echo tool with text {inputs[p]}. Then quote its exact returned value.'
        history = [{'role': 'user', 'content': prompt}]
        if p == 'chat':
            response = client.chat.completions.create(model=model, messages=history, tools=TOOLS_CHAT, **chat_opts)
            choice = response.choices[0]
            calls = choice.message.tool_calls or []
            assert choice.finish_reason == 'tool_calls' and len(calls) == 1, missing_callback(p, choice.message.content)
            call = calls[0]
            name, arguments = call.function.name, call.function.arguments
            history += [{'role': 'assistant', 'content': choice.message.content, 'tool_calls': [call.model_dump(exclude_none=True)]},
                        {'role': 'tool', 'tool_call_id': call.id, 'content': results[p]}]
        else:
            with client.responses.stream(model=model, input=history, tools=TOOLS_RESP, **resp_opts) as stream:
                response = stream.get_final_response()
            assert response.status == 'completed', f'{p}: incomplete tool-call stream'
            calls = [item for item in response.output if item.type == 'function_call']
            assert len(calls) == 1, missing_callback(p, response.output_text)
            call = calls[0]
            name, arguments = call.name, call.arguments
            history += [item.model_dump(exclude_none=True) for item in response.output]
            history.append({'type': 'function_call_output', 'call_id': call.call_id, 'output': results[p]})
        assert name == 'echo' and json.loads(arguments) == {'text': inputs[p]}, f'{p}: wrong tool or arguments'
        return history

    def finish(p, history):
        if p == 'chat':
            choice = client.chat.completions.create(model=model, messages=history, tools=TOOLS_CHAT, **chat_opts).choices[0]
            assert choice.finish_reason == 'stop', f'{p}: incomplete response'
            text = choice.message.content or ''
        else:
            response = client.responses.create(model=model, input=history, tools=TOOLS_RESP, **resp_opts)
            assert response.status == 'completed', f'{p}: incomplete response'
            text = response.output_text
        assert results[p] in text and not any(v in text for k, v in results.items() if k != p), f'{p}: incorrect client result'

    with ThreadPoolExecutor(max_workers=2) as pool:
        # Observe both first turns before supplying either result. Even when one
        # side omits the callback, finish the valid peer's pending echo call.
        started = {p: pool.submit(begin, p) for p in protocols}
        histories, failures = {}, {}
        for p, future in started.items():
            try:
                histories[p] = future.result()
            except Exception as exc:
                failures[p] = str(exc) if isinstance(exc, AssertionError) else type(exc).__name__
        finished = {p: pool.submit(finish, p, h) for p, h in histories.items()}
        for p, future in finished.items():
            try:
                future.result()
            except Exception as exc:
                failures[p] = str(exc) if isinstance(exc, AssertionError) else type(exc).__name__
        if failures:
            raise AssertionError({p: failures.get(p, 'round-trip OK') for p in protocols})


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--base', default='http://127.0.0.1:8741')
    p.add_argument('--key-file', default='.quickway/proxy.key')
    p.add_argument('--model', default='claude-sonnet-4-6')
    p.add_argument('--effort', choices=['none', 'low', 'medium', 'high'])
    suites = p.add_mutually_exclusive_group()
    suites.add_argument('--protocol', choices=['chat', 'responses'], help='Run only this protocol when isolating a live failure')
    suites.add_argument('--parallel-tools', action='store_true', help='Run only the two-client same-name tool isolation check (uses quota)')
    a = p.parse_args()
    client = OpenAI(base_url=a.base + '/v1', api_key=pathlib.Path(a.key_file).read_text().strip(), max_retries=0, timeout=180)
    if a.parallel_tools:
        parallel_tools(client, a.model, a.effort)
        print(json.dumps({'parallel_chat_responses_tool_roundtrip': True}))
        return
    passed = {}
    for protocol, check in [('chat', chat), ('responses', responses)]:
        if a.protocol and a.protocol != protocol:
            continue
        check(client, a.model, a.effort)
        passed[protocol] = True
        print(json.dumps({protocol: True}), flush=True)
    print(json.dumps({'model': a.model, 'effort': a.effort, **passed}))


if __name__ == '__main__':
    main()
