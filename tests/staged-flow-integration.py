"""Run the real Rust runtime and staged adapter with isolated, fake window I/O.
No production sessions, editors, or model endpoints are used by these tests.
"""
from pathlib import Path
import json, hashlib, tempfile, subprocess, time, os, shutil
ROOT=Path(__file__).resolve().parents[1]
EXE=ROOT/'target/draft-first/release/handoff-runtime.exe'
HIDE=getattr(subprocess,'CREATE_NO_WINDOW',0)
def dump(path,obj):
    path.parent.mkdir(parents=True,exist_ok=True)
    tmp=path.with_suffix('.test.tmp');tmp.write_text(json.dumps(obj,ensure_ascii=False),encoding='utf-8');os.replace(tmp,path)
def load(path):
    try:return json.loads(path.read_text(encoding='utf-8-sig'))
    except (OSError,ValueError):return {}
def wait_for(fn,seconds=20):
    end=time.monotonic()+seconds
    while time.monotonic()<end:
        v=fn()
        if v:return v
        time.sleep(.06)
    raise AssertionError('Condition timed out')
def setup(delay):
    root=Path(tempfile.mkdtemp(prefix='a2a-stage-test-'))
    for d in ('runtime/commands','runtime/requests','runtime/receipts','runtime/fixture','adapters/windows','adapters/claude','adapters/dsh','adapters/common'):(root/d).mkdir(parents=True)
    for f in ('draft-flow.ps1','draft-context.ps1','draft-primitives.ps1'):shutil.copy2(ROOT/'adapters/windows'/f,root/'adapters/windows'/f)
    for f in ('message-correlation.ps1','submit-evidence.ps1'):shutil.copy2(ROOT/'adapters/claude'/f,root/'adapters/claude'/f)
    shutil.copy2(ROOT/'adapters/common/a2a-config.ps1',root/'adapters/common/a2a-config.ps1')
    b={'claude_session':'cse_test','claude_window':'Test','dsh_session':'session-test'}
    dump(root/'runtime/bindings.json',b)
    dump(root/'runtime/config.json',{'enabled':True,'poll_seconds':1,'dispatch_delay_seconds':delay})
    dump(root/'runtime/message-templates.json',{'version':1,'to_dsh':{'prefix':'Read TASK.md.','suffix':''},'to_claude':{'prefix':'[','suffix':']'}})
    body='A completed DSH report.'
    d={'ok':True,'turn':7,'busy':False,'user_seq':11,'last_question_answer_seq':0,'result':{'turn':7,'reply':body,'end_seq':100,'hash':hashlib.sha256(body.encode()).hexdigest()}}
    c={'ok':True,'state':'replied','latest_user_index':5,'latest_user_hash':'human','ui_message_index':6,'tail_present':True,'reply_available':True,'reply_text':'Old instructions','session_id':'cse_test'}
    dump(root/'runtime/fixture/dsh.json',d);dump(root/'runtime/fixture/claude.json',c)
    key=hashlib.sha256(json.dumps(b,sort_keys=True,separators=(',',':')).encode()).hexdigest()
    dump(root/'runtime/workflow.json',{'schema':1,'binding_key':key,'phase':'waiting_dsh','dsh_after_seq':11,'dsh_user_seq':11,'dsh_answer_seq':0,'claude_anchor_index':5,'claude_anchor_hash':'human','claude_floor':6,'pending':None,'ledger':[]})
    (root/'adapters/dsh/observe.mjs').write_text("import{readFileSync}from'node:fs';console.log(readFileSync(process.argv[2]+'/runtime/fixture/dsh.json','utf8'));",encoding='utf-8')
    (root/'adapters/claude/observe.ps1').write_text("param($SessionId,$Title,[switch]$IncludeReply)\n[Console]::WriteLine([IO.File]::ReadAllText((Join-Path $PSScriptRoot '../../runtime/fixture/claude.json')))\n",encoding='utf-8-sig')
    ctx=root/'adapters/windows/draft-context.ps1'
    with ctx.open('a',encoding='utf-8') as f:f.write('''
# Isolated fixture overrides window discovery only; session guards remain production code.
function Dsh-Observe {Read-V1 (Join-Path $ProductRoot 'runtime/fixture/dsh.json')}
function Claude-Observe {Read-V1 (Join-Path $ProductRoot 'runtime/fixture/claude.json')}
function Bound-Composer($Dsh,$Observation){return @{fixture=$true}}
''')
    (root/'adapters/windows/composer-draft.ps1').write_text('''
# Fake window I/O for the real adapter; no UIA actions and no network.
function Trace($event){[IO.File]::AppendAllText((Join-Path $ProductRoot 'trace.jsonl'),((@{event=$event;at=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()}|ConvertTo-Json -Compress)+"`n"))}
function Read-ComposerText($m){$f=Join-Path $ProductRoot 'draft.txt';if(Test-Path $f){return [IO.File]::ReadAllText($f)};return ''}
function Test-ExactDraft($m,$text){return ((Read-ComposerText $m) -ceq $text)}
function Set-PlainOwnedDraft($m,$text,$before){
 if(Read-ComposerText $m){throw 'DRAFT_OCCUPIED'}
 & $before;[IO.File]::WriteAllText((Join-Path $ProductRoot 'draft.txt'),$text);Trace 'draft_written'
}
function Submit-VerifiedDraft($m,$text,$before){
 if(-not (Test-ExactDraft $m $text)){throw 'DRAFT_EDITED_SEND_CANCELLED'}
 & $before;Trace 'submitted';[IO.File]::WriteAllText((Join-Path $ProductRoot 'draft.txt'),'')
''',encoding='utf-8-sig')
    with (root/'adapters/windows/composer-draft.ps1').open('a',encoding='utf-8') as f:f.write('''
 $c=Claude-Observe;$c.latest_user_index=$script:r.claude_reply_index+1;$c.ui_message_index=$c.latest_user_index
 $c.latest_user_hash='owned-hash';$c|Add-Member -Force NoteProperty latest_user_body_hash (Get-PlainMessageHash $text)
 $c.state='awaiting_reply';$c.reply_available=$false
 Write-V1 (Join-Path $ProductRoot 'runtime/fixture/claude.json') $c
}
''')
    return root,b
def launch(root):
    err=(root/'stderr.txt').open('a',encoding='utf-8')
    p=subprocess.Popen([str(EXE),'--product-root',str(root),'--owner-pid',str(os.getpid())],stdout=subprocess.DEVNULL,stderr=err,creationflags=HIDE)
    return p,err
def trace(root):
    f=root/'trace.jsonl'
    return [json.loads(x) for x in f.read_text().splitlines()] if f.exists() else []
def run_case(kind,delay):
    root,b=setup(delay);p,err=launch(root)
    try:
        st=wait_for(lambda: (s if (s:=load(root/'runtime/state.json')).get('pending',{} ) and s['pending'].get('stage')=='draft_ready' else None))
        events=trace(root);assert [x['event'] for x in events]==['draft_written'],events
        written=events[0]['at'];deadline=st['pending']['deadline_ms'];assert deadline>=written+delay*1000
        assert p.poll() is None
        if kind=='cancel':
            dump(root/'runtime/commands/000-cancel.json',{'command':'cancel','bindings':b})
            wait_for(lambda:load(root/'runtime/state.json').get('pending') is None)
            time.sleep(delay+.5);assert not any(x['event']=='submitted' for x in trace(root))
            p.terminate();p.wait();err.close();p,err=launch(root);time.sleep(1.5)
            assert len(trace(root))==1,'Cancelled result was repasted on restart'
        elif kind=='edit':
            (root/'draft.txt').write_text('User edited this draft.',encoding='utf-8')
            wait_for(lambda:load(root/'runtime/state.json').get('phase')=='hold_send_uncertain',10)
            assert not any(x['event']=='submitted' for x in trace(root))
        else:
            # Clicking send again during countdown must not create a second write or submit.
            for i in range(2):dump(root/f'runtime/commands/000-send-{i}.json',{'command':'send_claude','bindings':b})
            wait_for(lambda:(load(root/'runtime/state.json').get('last_delivery') or {}).get('state')=='sent',delay+12)
            events=trace(root);assert [x['event'] for x in events]==['draft_written','submitted'],events
            assert events[1]['at']-written>=delay*1000,events
            time.sleep(1.3);assert len(trace(root))==2
        return {'case':kind,'passed':True,'trace':trace(root),'real_agent_messages_sent':0,'sandbox':str(root)}
    except BaseException:
        print('FAIL_STATE',load(root/'runtime/state.json'));print('FAIL_WORKFLOW',load(root/'runtime/workflow.json'))
        print('FAIL_STDERR',(root/'stderr.txt').read_text(encoding='utf-8'));raise
    finally:
        if p.poll() is None:p.terminate();p.wait(timeout=5)
        err.close()
if __name__=='__main__':
    results=[run_case('auto_normal',10),run_case('cancel',2),run_case('edit',2)]
    out=ROOT/'artifacts/staged-flow-integration.json';out.parent.mkdir(exist_ok=True)
    dump(out,{'results':results,'kind':'real_runtime_real_adapter_fake_window_io','messages_sent_to_agents':0})
    print(json.dumps({'passed':len(results),'results':results},ensure_ascii=False))
