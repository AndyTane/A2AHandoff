# Read-only adapter for the explicitly bound Cowork document. No clicks or keys.
param(
    [Parameter(Mandatory=$true)][string]$SessionId,
    [Parameter(Mandatory=$true)][string]$Title,
    [string]$ClaudeHost = '',
    [string]$ProductRoot = '',
    [switch]$IncludeReply
)
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
. (Join-Path $PSScriptRoot 'message-correlation.ps1')
. (Join-Path $PSScriptRoot '..\common\a2a-config.ps1')
# Claude identity stays <host>/cowork/<cse_id>. The host is configurable, but the
# path shape is not: the window title is presentation metadata only.
# NOTE: never name this parameter `Host` — PowerShell reserves `$Host` and binding
# would fail before the script body runs, producing no output at all.
$script:ClaudeHost = if ($ClaudeHost) {
    $ClaudeHost.Trim().ToLowerInvariant()
} elseif ($ProductRoot) {
    Get-A2aClaudeHost (Get-A2aRuntimeConfig -ProductRoot $ProductRoot)
} else {
    $script:A2aConfigDefaults.ClaudeHost
}
function Condition($type){return [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty,$type)}
$walk=[Windows.Automation.TreeWalker]::RawViewWalker
. (Join-Path $PSScriptRoot 'reply-body.ps1')
. (Join-Path $PSScriptRoot 'user-body.ps1')

try{
 if($SessionId -notmatch '^cse_[A-Za-z0-9_-]+$'){throw 'UNSUPPORTED_SESSION_ID'}
 $documents=@();$dc=Condition ([Windows.Automation.ControlType]::Document)
 foreach($p in @(Get-Process claude -ErrorAction SilentlyContinue|Where-Object {$_.MainWindowHandle -ne 0})){
  $root=[Windows.Automation.AutomationElement]::FromHandle($p.MainWindowHandle)
  foreach($doc in $root.FindAll([Windows.Automation.TreeScope]::Descendants,$dc)){
   $value=$null;$uri=$null
   if(-not $doc.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern,[ref]$value)){continue}
   if(-not [Uri]::TryCreate($value.Current.Value,[UriKind]::Absolute,[ref]$uri)){continue}
   if($uri.Host -ne $script:ClaudeHost -or $uri.AbsolutePath -cne ('/cowork/'+$SessionId)){continue}
   $documents+=@{Doc=$doc;Value=$value;Pid=$p.Id}
  }
 }
 if($documents.Count -ne 1){throw 'TARGET_DOCUMENT_UNAVAILABLE: open the bound Claude conversation.'}
 $selected=$documents[0];$doc=$selected.Doc;$textPattern=$null
 if(-not $doc.TryGetCurrentPattern([Windows.Automation.TextPattern]::Pattern,[ref]$textPattern)){throw 'DOCUMENT_TEXT_UNAVAILABLE'}
 $all=$doc.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)
 $chat=@($all|Where-Object {$_.Current.Name -ceq 'Chat messages' -and $_.Current.ControlType -eq [Windows.Automation.ControlType]::Group})
 if($chat.Count -ne 1){throw 'CHAT_STRUCTURE_UNSUPPORTED'}
 $gc=Condition ([Windows.Automation.ControlType]::Group);$tc=Condition ([Windows.Automation.ControlType]::Text)
 $messages=@()
 foreach($group in $chat[0].FindAll([Windows.Automation.TreeScope]::Descendants,$gc)){
  if($group.Current.Name -notmatch '^Message (\d+) of (\d+)$'){continue}
  $index=[int]$Matches[1];$total=[int]$Matches[2];$role='unknown'
  foreach($element in $group.FindAll([Windows.Automation.TreeScope]::Descendants,$tc)){
   if($element.Current.ClassName -notmatch 'sr-only'){continue}
   if($element.Current.Name -match '^Claude responded:'){$role='assistant';break}
   if($element.Current.Name -match '^You said:'){$role='user';break}
  }
  $messages+=[pscustomobject]@{Index=$index;Total=$total;Role=$role;Node=$group}
 }
 if($messages.Count -eq 0){throw 'NO_LOADED_MESSAGES'}
 $messages=@($messages|Sort-Object Index);$last=$messages[-1]
 if(@($messages|Group-Object Index|Where-Object {$_.Count -gt 1}).Count){throw 'AMBIGUOUS_MESSAGE_ORDER'}
 $atTail=$last.Index -eq $last.Total
 $busy=@($all|Where-Object {$_.Current.ControlType -eq [Windows.Automation.ControlType]::Button -and $_.Current.IsEnabled -and -not $_.Current.IsOffscreen -and $_.Current.Name -match '^(Stop|Stop response|Stop responding|Stop task|Cancel task)$'}).Count -gt 0
 $needsInput=@($all|Where-Object {$_.Current.ControlType -eq [Windows.Automation.ControlType]::Button -and $_.Current.IsEnabled -and -not $_.Current.IsOffscreen -and $_.Current.Name -match '^(Allow once|Allow always|Approve|Deny)$'}).Count -gt 0
 $finished=@($all|Where-Object {$_.Current.ControlType -eq [Windows.Automation.ControlType]::Text -and $_.Current.Name -ceq 'Claude finished the response'}).Count -gt 0
 $finalToolbar=@($last.Node.FindAll([Windows.Automation.TreeScope]::Descendants,(Condition ([Windows.Automation.ControlType]::ToolBar)))|Where-Object {$_.Current.Name -ceq 'Message actions'}).Count -gt 0
 $user=@($messages|Where-Object {$_.Role -eq 'user' -and $_.Index -lt $last.Index}|Select-Object -Last 1)
 $state=if($needsInput){'needs_input'}elseif($busy){'running'}elseif(-not $atTail){'ui_tail_unavailable'}elseif($last.Role -eq 'assistant' -and $finished -and $finalToolbar){'replied'}elseif($last.Role -eq 'user'){'awaiting_reply'}else{'ui_state_unconfirmed'}
 $reply='';$age=$null
 if($state -eq 'replied' -and $IncludeReply){
  $reply=Read-ResponseBody $last.Node $textPattern
  if(-not $reply){throw 'REPLY_BODY_UNAVAILABLE'}
  if($reply.Length -gt 2000000){throw 'REPLY_TOO_LARGE: not truncated.'}
  if((Read-ResponseBody $last.Node $textPattern) -cne $reply){throw 'REPLY_CHANGED_DURING_READ'}
 }
 foreach($bar in $last.Node.FindAll([Windows.Automation.TreeScope]::Descendants,(Condition ([Windows.Automation.ControlType]::ToolBar)))){
  $ageTexts=@($bar.FindAll([Windows.Automation.TreeScope]::Descendants,$tc)|Where-Object {$_.Current.Name -match '^(.+ ago|Just now|Yesterday)$'})
  if($ageTexts.Count){$age=$ageTexts[-1].Current.Name}
 }
 if(([uri]$selected.Value.Current.Value).AbsolutePath -cne ('/cowork/'+$SessionId)){throw 'DOCUMENT_CHANGED_DURING_READ'}
 $tailNow=@($chat[0].FindAll([Windows.Automation.TreeScope]::Descendants,$gc)|Where-Object {$_.Current.Name -match '^Message \d+ of \d+$'})
 if($tailNow.Count -eq 0 -or $tailNow[-1].Current.Name -cne ('Message '+$last.Index+' of '+$last.Total)){throw 'MESSAGE_ORDER_CHANGED_DURING_READ'}
 $human=@($messages|Where-Object Role -eq 'user'|Select-Object -Last 1)
 $humanIndex=0;$humanHash='';$bodyHash=''
 if($human.Count){
  $humanIndex=$human[0].Index
  $humanText=$textPattern.RangeFromChild($human[0].Node).GetText(-1)
  foreach($bar in $human[0].Node.FindAll([Windows.Automation.TreeScope]::Descendants,(Condition ([Windows.Automation.ControlType]::ToolBar)))){
   $dynamic=$textPattern.RangeFromChild($bar).GetText(-1)
   if($dynamic){$humanText=$humanText.Replace($dynamic,'')}
  }

  # The anchor hash must come from the bounded body range, not the raw node text.
  # The raw range carries the collapse indicator, the expand control and the action
  # bar, all of which change as the view re-renders; hashing them made the anchor
  # drift while the content was untouched, so the manual-intervention guard fired
  # spuriously and auto handoff paused itself after a successful delivery.
  $humanBody=Read-UserMessageBody $human[0].Node $textPattern
  $bodyHash=Get-PlainMessageHash $humanBody
  $humanHash=$bodyHash
  if(-not $humanBody.Trim()){
   # Degraded fallback: no bounded body was recoverable, so fall back to the raw
   # range rather than publishing an anchor that can never match anything.
   $normalized=($humanText -replace '\r\n?','\n' -replace '\s+',' ').Trim()
   $sha=[Security.Cryptography.SHA256]::Create();try{$humanHash=([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($normalized)))).Replace('-','').ToLowerInvariant()}finally{$sha.Dispose()}
  }
 }
 $result=[ordered]@{latest_user_index=$humanIndex;latest_user_hash=$humanHash;latest_user_body_hash=$bodyHash;ok=$true;source='claude_desktop_uia';evidence='uia_document_snapshot';session_id=$SessionId;title=$doc.Current.Name;observed_at=[DateTime]::UtcNow.ToString('o');state=$state;ui_message_index=$last.Index;ui_total_messages=$last.Total;ui_user_message_index=$(if($user.Count){$user[0].Index}else{$null});message_index_scope='current_ui_snapshot';tail_present=$atTail;reply_available=($state -eq 'replied');reply_is_current_turn=($state -eq 'replied' -and $user.Count -eq 1);correlation='ui_message_order';source_timestamp=$null;displayed_age=$age;official_hook_received=$false;task_success=$null;app_pid=$selected.Pid;no_ui_actions=$true}
 if($IncludeReply -and $reply){$result.reply_text=$reply;$result.reply_format='plain_text_from_uia';$result.non_text_content_present=$reply.Contains([string][char]0xfffc)}
 [Console]::WriteLine(($result|ConvertTo-Json -Depth 8 -Compress))
}catch{
 [Console]::WriteLine((@{ok=$false;source='claude_desktop_uia';error=$_.Exception.Message;session_id=$SessionId;no_ui_actions=$true}|ConvertTo-Json -Compress));exit 1
}
