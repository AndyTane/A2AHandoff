// Native read-only session observer. No CLI process, model call, or V0 runtime dependency.
import {readFileSync,readdirSync,existsSync} from 'node:fs';
import {join,resolve} from 'node:path';
import {createHash} from 'node:crypto';
import {readRows} from './read-log.mjs';
import {readRuntimeConfig,dshDataHome,workspace} from '../common/a2a-config.mjs';
export const hash=text=>createHash('sha256').update(text.replace(/\r\n?/g,'\n').trim()).digest('hex');
export function observe(product){
 const read=p=>JSON.parse(readFileSync(p,'utf8').replace(/^\uFEFF/,''));
 const cfg=readRuntimeConfig(product), b=read(join(product,'runtime/bindings.json'));
 const sid=b.dsh_session;if(!/^session-[A-Za-z0-9_-]+$/.test(sid))throw Error('INVALID_DSH_BINDING');
 const home=dshDataHome(cfg);
 if(!home)throw Error('DSH_DATA_HOME_UNSET: set the DSH data directory in Settings.');
 const ws=workspace(cfg);
 const p=read(join(home,'storages/session_projcache/sessions',sid+'.json'));
 if(ws&&resolve(p.record.identity.cwd).toLowerCase()!==resolve(ws).toLowerCase())throw Error('DSH_WORKSPACE_MISMATCH');
 const parent=readdirSync(join(home,'sessions')).find(d=>existsSync(join(home,'sessions',d,sid,'session.v3.jsonl.zstd')));
 if(!parent)throw Error('DSH_NATIVE_LOG_UNAVAILABLE');
 const log=join(home,'sessions',parent,sid,'session.v3.jsonl.zstd');
 let header,latestUser=null,lastAnswerSeq=0,lastSeq=0,completed=null,currentTurn=0,openSeq=null,goal=null,goalSeq=0;
 const turns=new Map(),asks=new Map();
 for(const e of readRows(log)){
  lastSeq=Math.max(lastSeq,e.seq||0);if(e.type==='session'){header=e;continue;}
  const d=e.data||{},m=d.message||d;
  if(e.type==='turn/start'){currentTurn=d.turn;openSeq=e.seq;}
  if(e.type==='turn/end'&&d.turn===currentTurn)openSeq=null;
  if(e.type==='goal/change'){goalSeq=e.seq;goal=d.operation==='clear'?null:d.goal||goal;}
  if(e.type==='user/message'&&m.source?.kind==='user')latestUser={seq:e.seq,text:(m.content||[]).filter(x=>x.type==='text').map(x=>x.text).join(''),rpc_id:m.source.rpcId||''};
  if(e.type==='assistant/message'&&!(m.content||[]).some(x=>x.type==='tool-call')){
   const text=(m.content||[]).filter(x=>x.type==='text').map(x=>x.text).join('').trim();
   if(text)turns.set(d.turn,{turn:d.turn,reply:text,message_seq:e.seq,user_seq:latestUser?.seq||0});
  }
  if(e.type==='tool/call'&&d.name==='ask_user_question')asks.set(d.callId,{call_id:d.callId,seq:e.seq,arguments:d.arguments});
  if(e.type==='tool/result'){const id=m.source?.callId||d.callId;if(asks.has(id)){asks.delete(id);lastAnswerSeq=e.seq;}}
  if(e.type==='turn/end'&&['completed','blocked'].includes(d.reason?.kind)&&turns.has(d.turn))completed={...turns.get(d.turn),end_seq:e.seq,reason:d.reason.kind};
  if(e.type==='turn/end')turns.delete(d.turn);
 }
 if(header?.id!==sid)throw Error('DSH_NATIVE_IDENTITY_MISMATCH');
 const rows=p.record.rows, boundary=rows.turnBoundary.val;
 // A newer human prompt makes every earlier result ineligible, even if it is the latest completed turn.
 if(completed&&completed.user_seq!==(latestUser?.seq||0))completed=null;
 if(completed)completed.hash=hash(completed.reply);
 return {ok:true,session_id:sid,title:rows.title.val,cwd:p.record.identity.cwd,
  turn:currentTurn,busy:openSeq!==null,open_seq:openSeq,
  last_seq:lastSeq,user_seq:latestUser?.seq||0,user_hash:hash(latestUser?.text||''),user_request_id:latestUser?.rpc_id||'',
  last_question_answer_seq:lastAnswerSeq,pending_question:[...asks.values()].at(-1)||null,
  goal,goal_seq:goalSeq,result:completed,source:'native_session_log'};
}
if(process.argv[1]&&resolve(process.argv[1])===resolve(new URL(import.meta.url).pathname.replace(/^\/(\w:)/,'$1'))){
 try{console.log(JSON.stringify(observe(process.argv[2])));}catch(e){console.log(JSON.stringify({ok:false,error:e.message}));process.exitCode=1;}
}
