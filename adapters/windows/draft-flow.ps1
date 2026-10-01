# Stage and commit are separate invocations; this process never owns a countdown loop.
param([Parameter(Mandatory=$true)][string]$ProductRoot,[Parameter(Mandatory=$true)][string]$RequestFile,[Parameter(Mandatory=$true)][ValidateSet('Prepare','Commit')][string]$Operation)
. "$PSScriptRoot\draft-context.ps1"
. "$PSScriptRoot\composer-draft.ps1"
. (Join-Path $PSScriptRoot "..\claude\submit-evidence.ps1")
function Now-Ms {return [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()}
function Assert-ActiveDelivery {
 $w=Read-V1 (Join-Path $ProductRoot 'runtime/workflow.json')
 if($null -eq $w.pending -or $w.pending.id -cne $script:r.id){throw 'DELIVERY_CANCELLED'}
 $b=Read-V1 (Join-Path $ProductRoot 'runtime/bindings.json')
 if($b.claude_session -cne $script:r.claude_session -or $b.dsh_session -cne $script:r.dsh_session){throw 'BINDING_CHANGED'}
 $cfg=Read-V1 (Join-Path $ProductRoot 'runtime/config.json')
 if(-not $script:r.manual -and -not $cfg.enabled){throw 'AUTOMATION_PAUSED'}
 foreach($file in @(Get-ChildItem (Join-Path $ProductRoot 'runtime/commands') -Filter '*.json' -File)){
  $ctl=Read-V1 $file.FullName
  if($ctl.command -in @('cancel','toggle') -and $ctl.bindings.claude_session -ceq $b.claude_session -and $ctl.bindings.dsh_session -ceq $b.dsh_session){throw 'CANCEL_OR_PAUSE_REQUESTED'}
 }
}
function Receipt($state,$anchor=$null){
 Write-V1 $script:receipt ([ordered]@{id=$script:r.id;flow='draft-first-v1';state=$state;direction=$script:r.direction;source_hash=$script:r.source_hash;source_seq=$script:r.source_seq;source_turn=$script:r.source_turn;claude_session=$script:r.claude_session;dsh_session=$script:r.dsh_session;outgoing_hash=(Hash-Message $script:r.text);at_ms=(Now-Ms);draft_ready_at_ms=$script:readyAt;deadline_ms=$script:deadline;anchor=$anchor})
}
$mutex=[Threading.Mutex]::new($false,'Local\A2AHandoff.V1.MessageIO');$owned=$false
$script:readyAt=0;$script:deadline=0;$script:receipt=''
try{
 try{$owned=$mutex.WaitOne(1000)}catch [Threading.AbandonedMutexException]{$owned=$true}
 if(-not $owned){throw 'MESSAGE_IO_BUSY'}
 $script:r=Read-V1 $RequestFile;$script:b=Read-V1 (Join-Path $ProductRoot 'runtime/bindings.json')
 if($script:r.id -notmatch '^[a-f0-9]{24}$' -or $script:r.message_contract -cne 'configured-plain-v2'){throw 'INVALID_DELIVERY_REQUEST'}
 if([string]::IsNullOrWhiteSpace($script:r.text)){throw 'EMPTY_INSTRUCTION_NOT_SENT'}
 $script:receipt=Join-Path $ProductRoot ('runtime/receipts/'+$script:r.id+'.json')
 $previous=if(Test-Path -LiteralPath $script:receipt){Read-V1 $script:receipt}else{$null}
 if($null -ne $previous){
  if((Get-Field $previous 'flow' '') -cne 'draft-first-v1' -or $previous.state -ne 'draft_ready'){throw 'DELIVERY_ALREADY_ATTEMPTED_NO_RETRY'}
  if($previous.outgoing_hash -cne (Hash-Message $script:r.text)){throw 'DRAFT_REQUEST_CHANGED'}
  $script:readyAt=$previous.draft_ready_at_ms;$script:deadline=$previous.deadline_ms
 }
 Assert-ActiveDelivery
 $fresh=Assert-Fresh;$toDsh=$script:r.direction -eq 'CLAUDE_TO_DSH'
 $m=Bound-Composer $toDsh $fresh.Dsh
 if($toDsh){
  $cfg=Read-V1 (Join-Path $ProductRoot 'runtime/config.json')
  $same=@(Get-ChildItem (Join-Path $cfg.dsh_data_home 'storages/session_projcache/sessions') -Filter '*.json' -File|ForEach-Object {try{$j=Read-V1 $_.FullName;if($j.record.rows.title.val -ceq $fresh.Dsh.title){$_.BaseName}}catch{}})
  if($same.Count -ne 1 -or $same[0] -cne $script:b.dsh_session){throw 'DSH_TITLE_IDENTITY_NOT_UNIQUE'}
 }
 if($Operation -eq 'Prepare'){
  if($null -ne $previous){
   if(-not (Test-ExactDraft $m $script:r.text)){throw 'DRAFT_EDITED_SEND_CANCELLED'}
  }else{
   Set-PlainOwnedDraft $m $script:r.text {Assert-ActiveDelivery;Receipt 'draft_write_attempted'}
  }
  Assert-ActiveDelivery
  $script:readyAt=Now-Ms
  # The countdown exists so an AUTOMATIC handoff can be cancelled before it goes
  # out, so it is clamped to at least one second. A manual click is already the
  # confirmation, so it carries no delay at all and dispatches on the next tick.
  if($script:r.manual){$delay=0}else{$delay=[Math]::Max(1,[Math]::Min(120,[int]$script:r.dispatch_delay_seconds))}
  $script:deadline=$script:readyAt+$delay*1000
  Receipt 'draft_ready'
  [Console]::WriteLine((@{ok=$true;state='draft_ready';draft_ready_at_ms=$script:readyAt;deadline_ms=$script:deadline;messages_sent=0}|ConvertTo-Json -Compress))
  exit 0
 }
 if($null -eq $previous -or $previous.state -ne 'draft_ready'){throw 'DRAFT_NOT_STAGED'}
 if((Now-Ms) -lt [Math]::Max([long]$script:deadline,[long]$script:r.deadline_ms)){throw 'COUNTDOWN_NOT_FINISHED'}
 Submit-VerifiedDraft $m $script:r.text {Assert-ActiveDelivery;Receipt 'send_attempted'}
 $anchor=$null
 for($i=0;$i -lt 10;$i++){
  Start-Sleep -Milliseconds 200
  if($toDsh){
   $after=Dsh-Observe
   if($after.ok -and $after.user_seq -gt $script:r.dsh_user_seq -and $after.user_hash -ceq (Hash-Message $script:r.text)){$anchor=@{dsh_user_seq=$after.user_seq;dsh_turn=$after.turn;dsh_answer_seq=$after.last_question_answer_seq};break}
  }else{
   $after=Claude-Observe
   $empty=$false
   try{$current=Bound-Composer $false $after;$empty=[string]::IsNullOrWhiteSpace((Read-ComposerText $current))}catch{}
   $exact=Test-PlainClaudeReceipt $after $script:r $script:r.text
   if($exact -or (Test-NativeSubmitReceipt $after $script:r $true $empty)){
    $anchor=@{claude_user_index=$after.latest_user_index;claude_user_hash=$after.latest_user_hash;evidence=$(if($exact){'full_body_hash'}else{'native_submit_exact_draft_next_user_empty_composer'})};break
   }
  }
 }
 if($null -eq $anchor){Receipt 'submit_uncertain';throw 'SUBMIT_UNCONFIRMED_NO_AUTOMATIC_RETRY'}
 Receipt 'sent' $anchor
 [Console]::WriteLine((@{ok=$true;state='sent';id=$script:r.id;anchor=$anchor}|ConvertTo-Json -Depth 8 -Compress))
}catch{
 $problem=$_.Exception.Message
 # Never transform a paste failure into a request for a second user confirmation.
 if($script:receipt -and (Test-Path -LiteralPath $script:receipt)){
  $last=Read-V1 $script:receipt
  if($last.state -eq 'draft_ready' -and $problem -ne 'COUNTDOWN_NOT_FINISHED'){Receipt 'cancelled_before_send'}
  elseif($last.state -eq 'draft_write_attempted'){Receipt 'draft_unverified'}
 }
 [Console]::WriteLine((@{ok=$false;error=$problem;state='hold'}|ConvertTo-Json -Compress));exit 1
}finally{if($owned){$mutex.ReleaseMutex()};$mutex.Dispose()}
