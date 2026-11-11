"""Opt-in live checks. Calls the running local proxy and spends a few Quick tokens."""
import argparse, json, pathlib, urllib.request, urllib.error

def post(base,key,body,effort=None):
    headers={'x-api-key':key,'Content-Type':'application/json'}
    if effort is not None:headers['x-quickway-thinking-effort']=effort
    request=urllib.request.Request(base+'/v1/messages',data=json.dumps(body).encode(),headers=headers)
    try:
        with urllib.request.urlopen(request,timeout=120) as response:
            if not body.get('stream'):return json.load(response)
            events=[]
            for line in response:
                if line.startswith(b'data: '):events.append(json.loads(line[6:]))
            errors=[e for e in events if e.get('type')=='error']
            if errors:raise RuntimeError(json.dumps(errors))
            assert events[-1]['type']=='message_stop', 'Missing clean stream completion'
            return events
    except urllib.error.HTTPError as e:raise RuntimeError(str(e.code)+' '+e.read().decode()) from None

def assemble(events):
    blocks=[];parts={};stop=None
    for e in events:
        if e['type']=='content_block_start':blocks.append(e['content_block'])
        if e['type']=='content_block_delta':
            i=e['index'];d=e['delta'];t=d['type']
            if t=='text_delta':blocks[i]['text']+=d['text']
            if t=='input_json_delta':parts[i]=parts.get(i,'')+d['partial_json']
            if t=='thinking_delta':blocks[i]['thinking']+=d['thinking']
            if t=='signature_delta':blocks[i]['signature']+=d['signature']
        if e['type']=='message_delta':stop=e['delta']['stop_reason']
    for i,s in parts.items():blocks[i]['input']=json.loads(s)
    return blocks,stop

def main():
    p=argparse.ArgumentParser();p.add_argument('--base',default='http://127.0.0.1:8741');p.add_argument('--key-file',default='.quickway/proxy.key');p.add_argument('--model',default='claude-opus-4-6');p.add_argument('--tools',action='store_true');p.add_argument('--auto-tools',action='store_true');p.add_argument('--thinking-effort',choices=['off','low','medium','high']);a=p.parse_args()
    key=pathlib.Path(a.key_file).read_text().strip()
    request={'model':a.model,'max_tokens':4096 if a.thinking_effort else 1024,'messages':[{'role':'user','content':'Reply exactly QUICKWAY_OK.'}]}
    nonstream=post(a.base,key,request,a.thinking_effort);assert nonstream['stop_reason']=='end_turn';assert 'QUICKWAY_OK' in str(nonstream['content'])
    request['stream']=True
    blocks,stop=assemble(post(a.base,key,request,a.thinking_effort));assert stop=='end_turn';assert 'QUICKWAY_OK' in str(blocks)
    report={'model':a.model,'nonstream':True,'stream':True,'actual_model':nonstream['model']}
    if a.thinking_effort:report['thinking_effort']=a.thinking_effort
    if a.tools:
        for streaming in (False,True):
            request['stream']=streaming
            request['messages']=[{'role':'user','content':'Call the echo tool with text hello. Then quote the exact tool response verbatim, even if it differs from the requested text.'}]
            request['tools']=[{'name':'echo','description':'Return the supplied text.','input_schema':{'type':'object','properties':{'text':{'type':'string'}},'required':['text']}}]
            request['tool_choice']={'type':'auto'} if a.auto_tools else {'type':'tool','name':'echo'}
            result=post(a.base,key,request,a.thinking_effort)
            blocks,stop=assemble(result) if streaming else (result['content'],result['stop_reason'])
            assert stop=='tool_use', (streaming,stop)
            tool=next(b for b in blocks if b['type']=='tool_use');assert tool['name']=='echo' and isinstance(tool['input'],dict)
            request['messages'] += [{'role':'assistant','content':blocks},{'role':'user','content':[{'type':'tool_result','tool_use_id':tool['id'],'content':'ECHO_ROUNDTRIP_OK'}]}]
            request['tool_choice']={'type':'auto'}
            result=post(a.base,key,request,a.thinking_effort)
            blocks,stop=assemble(result) if streaming else (result['content'],result['stop_reason'])
            assert stop=='end_turn', (streaming,stop,blocks);assert 'ECHO_ROUNDTRIP_OK' in str(blocks), blocks
            report['tool_roundtrip' if streaming else 'nonstream_tool_roundtrip']=True
    print(json.dumps(report))

if __name__=='__main__':main()
