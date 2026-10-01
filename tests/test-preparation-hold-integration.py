"""Real runtime, fake editor: occupied input must hold once, then allow manual retry."""
from pathlib import Path
import importlib.util,time,json
root=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('flow',root/'tests/staged-flow-integration.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
r,b=m.setup(2)
f=r/'adapters/windows/composer-draft.ps1';s=f.read_text(encoding='utf-8-sig')
s=s.replace(" if(Read-ComposerText $m){throw 'DRAFT_OCCUPIED'}"," Trace 'prepare_called';if(Read-ComposerText $m){throw 'DRAFT_OCCUPIED_PRESERVED'}")
f.write_text(s,encoding='utf-8-sig');(r/'draft.txt').write_text('Existing user draft',encoding='utf-8')
p,err=m.launch(r)
try:
    m.wait_for(lambda:m.load(r/'runtime/state.json').get('phase')=='hold_preparation')
    time.sleep(3.5)
    assert [x['event'] for x in m.trace(r)]==['prepare_called']
    assert m.load(r/'runtime/workflow.json')['ledger']==[]
    assert (r/'draft.txt').read_text()=='Existing user draft'
    p.terminate();p.wait();err.close();p,err=m.launch(r);time.sleep(1.5)
    assert m.load(r/'runtime/state.json')['phase']=='hold_preparation'
    assert len(m.trace(r))==1
    (r/'draft.txt').write_text('',encoding='utf-8')
    m.dump(r/'runtime/commands/001.json',{'command':'send_claude','bindings':b})
    m.wait_for(lambda:(m.load(r/'runtime/state.json').get('last_delivery') or {}).get('state')=='sent')
    assert [x['event'] for x in m.trace(r)]==['prepare_called','prepare_called','draft_written','submitted']
    print(json.dumps({'occupied_draft_preserved':True,'retry_stopped':True,'restart_hold_preserved':True,'manual_retry_not_blocked_by_dedup':True,'real_messages_sent':0}))
finally:
    if p.poll() is None:p.terminate();p.wait(timeout=5)
    err.close()
