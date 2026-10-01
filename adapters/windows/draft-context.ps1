# V1 message I/O. A durable intent receipt is written immediately before the single
# submit. Submission is never a bare Enter: DSH is written through a guarded clipboard
# paste, Claude through its composer's own send control.
. "$PSScriptRoot\draft-primitives.ps1"
. "$PSScriptRoot\..\common\a2a-config.ps1"
. (Join-Path $PSScriptRoot '..\claude\message-correlation.ps1')
function Read-V1($p){return ([IO.File]::ReadAllText($p,[Text.Encoding]::UTF8)|ConvertFrom-Json)}
function Write-V1($p,$v){$tmp=$p+'.'+$PID+'.tmp';[IO.File]::WriteAllText($tmp,($v|ConvertTo-Json -Depth 12),[Text.UTF8Encoding]::new($false));if(Test-Path -LiteralPath $p){[IO.File]::Replace($tmp,$p,($p+'.previous'))}else{[IO.File]::Move($tmp,$p)}}
function Dsh-Observe {return ((& node.exe (Join-Path $ProductRoot 'adapters\dsh\observe.mjs') $ProductRoot)|ConvertFrom-Json)}
function Claude-Observe {return ((& powershell.exe -NoProfile -STA -ExecutionPolicy Bypass -File (Join-Path $ProductRoot 'adapters\claude\observe.ps1') -SessionId $script:b.claude_session -Title $script:b.claude_window -ProductRoot $ProductRoot -IncludeReply)|ConvertFrom-Json)}
function Bound-Composer($Dsh,$Observation){
 $probe=[Collections.Generic.List[string]]::new();$found=@();$dc=Get-UiCondition ([Windows.Automation.ControlType]::Document);$ec=Get-UiCondition ([Windows.Automation.ControlType]::Edit)
 $ioConfig=Get-A2aRuntimeConfig -ProductRoot $ProductRoot
 # Every environment value comes from configuration; nothing about the machine is
 # hard-coded here (see adapters/common/a2a-config.ps1).
 $names=if($Dsh){@(Get-A2aDshBrowserProcesses $ioConfig)}else{@('claude')}
 $origin=if($Dsh){Get-A2aDshWebOrigin $ioConfig}else{$null}
 $titlePattern=Get-A2aDshPageTitlePattern $ioConfig
 $claudeHost=Get-A2aClaudeHost $ioConfig
 $windows=@();$seenDocuments=[Collections.Generic.HashSet[string]]::new();$seenInputs=[Collections.Generic.HashSet[string]]::new()
 if($Dsh){
  foreach($root in [Windows.Automation.AutomationElement]::RootElement.FindAll([Windows.Automation.TreeScope]::Children,[Windows.Automation.Condition]::TrueCondition)){
   $proc=Get-Process -Id $root.Current.ProcessId -ErrorAction SilentlyContinue
   if($null -eq $proc -or $proc.ProcessName -notin $names -or $root.Current.NativeWindowHandle -eq 0){continue}
   if($root.Current.Name -notlike ('*'+$titlePattern+'*')){continue}
   $windows+=@{Root=$root;Window=[pscustomobject]@{Id=$proc.Id;MainWindowHandle=[IntPtr]$root.Current.NativeWindowHandle}}
  }
 }else{foreach($win in @(Get-Process -Name $names -ErrorAction SilentlyContinue|Where-Object {$_.MainWindowHandle -ne 0})){$windows+=@{Window=$win;Root=[Windows.Automation.AutomationElement]::FromHandle($win.MainWindowHandle)}}}
 foreach($entry in $windows){
  $win=$entry.Window;$root=$entry.Root
  # Chromium's first document is the active top-level tab. Ignore repeated
  # descendant aliases of that same main frame; distinct windows remain distinct.
  if($Dsh){$primary=$root.FindFirst([Windows.Automation.TreeScope]::Descendants,$dc);$documents=@();if($null -ne $primary){$documents=@($primary)}}
  else{$documents=$root.FindAll([Windows.Automation.TreeScope]::Descendants,$dc)}
  foreach($doc in $documents){
   $documentIdentity=($doc.GetRuntimeId() -join ':')
   if(-not $seenDocuments.Add($documentIdentity)){continue}
   $probe.Add(("doc="+$(if($doc.Current.Name -like ('*'+$titlePattern+'*')){$doc.Current.Name}else{"other"})+";offscreen="+$doc.Current.IsOffscreen))
   if($doc.Current.IsOffscreen){continue};$dv=$null;$uri=$null
   if(-not $doc.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern,[ref]$dv)-or -not [Uri]::TryCreate($dv.Current.Value,[UriKind]::Absolute,[ref]$uri)){continue}
   if($Dsh -and $doc.Current.Name -like ('*'+$titlePattern+'*')){$probe.Add(('title='+$doc.Current.Name+';expected='+$Observation.title+';prefix='+$doc.Current.Name.StartsWith($Observation.title+' ')+';host='+$uri.Host+';port='+$uri.Port))}
   if($Dsh){if($uri.Scheme -cne $origin.Scheme -or $uri.Host -cne $origin.Host -or $uri.Port -ne $origin.Port -or -not $doc.Current.Name.StartsWith($Observation.title+' ') -or $doc.Current.Name -notlike ('*'+$titlePattern+'*')){continue}}
   else{if($uri.Host -ne $claudeHost -or $uri.AbsolutePath -cne ('/cowork/'+$script:b.claude_session)){continue}}
   foreach($edit in $doc.FindAll([Windows.Automation.TreeScope]::Descendants,$ec)){
    if($edit.Current.IsOffscreen -or -not $edit.Current.IsEnabled){continue}
    if($Dsh){if($edit.Current.Name -notlike '*@*' -and $edit.Current.ClassName -notmatch '_input$'){continue}}elseif($edit.Current.Name -notlike '*prompt*Claude*'){continue}
    $inputIdentity=($edit.GetRuntimeId() -join ':')
    if(-not $seenInputs.Add($inputIdentity)){continue}
    $vp=$null;if($edit.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern,[ref]$vp)){$found+=@{Window=$win;Document=$doc;Edit=$edit;Value=$vp;Title=$doc.Current.Name;TitlePattern=$titlePattern;DocumentHash=(Get-DocHash $doc)}}
   }
  }
 }
 # The active tab is all UI Automation can see, so when the window is showing a DIFFERENT
 # session than the one being observed, this is the failure. Naming the expected session
 # title in the message is the difference between "it is stuck" and "bring that tab
 # forward" - the earlier message only listed what it saw.
 $expects=if($Dsh){';expects='+$Observation.title}else{''}
 if($found.Count -ne 1){throw ('TARGET_WINDOW_UNAVAILABLE_OR_AMBIGUOUS; dsh='+$Dsh+';windows='+$windows.Count+';matches='+$found.Count+$expects+';docs='+($probe -join '|'))}
 return $found[0]
}
function Receipt($state,$anchor=$null){Write-V1 $script:receipt ([ordered]@{id=$script:r.id;state=$state;direction=$script:r.direction;source_hash=$script:r.source_hash;source_seq=$script:r.source_seq;source_turn=$script:r.source_turn;claude_session=$script:r.claude_session;dsh_session=$script:r.dsh_session;at=[DateTime]::UtcNow.ToString('o');anchor=$anchor})}
function Assert-Fresh {
 $cfg=Read-V1 (Join-Path $ProductRoot 'runtime/config.json')
 if(-not $script:r.manual -and -not $cfg.enabled){throw 'AUTOMATION_PAUSED'}
 $expectedTemplates=Get-Field $script:r 'message_templates' $null
 if($null -ne $expectedTemplates){
  $actualTemplates=Read-V1 (Join-Path $ProductRoot 'runtime/message-templates.json')
  if($actualTemplates.version -ne $expectedTemplates.version){throw 'MESSAGE_TEMPLATES_CHANGED'}
  foreach($direction in @('to_dsh','to_claude')){foreach($part in @('prefix','suffix')){
   if($actualTemplates.$direction.$part -cne $expectedTemplates.$direction.$part){throw 'MESSAGE_TEMPLATES_CHANGED'}
  }}
 }

 foreach($file in @(Get-ChildItem (Join-Path $ProductRoot 'runtime/commands') -Filter '*.json' -File -ErrorAction SilentlyContinue)){
  try{$control=Read-V1 $file.FullName}catch{continue}
  if($control.command -in @('cancel','toggle') -and $control.bindings.dsh_session -ceq $script:r.dsh_session -and $control.bindings.claude_session -ceq $script:r.claude_session){throw 'CANCEL_OR_PAUSE_REQUESTED'}
 }

 $binding=Read-V1 (Join-Path $ProductRoot 'runtime\bindings.json')
 foreach($key in @('claude_session','claude_window','dsh_session')){if($binding.$key -cne $script:r.$key){throw 'BINDING_CHANGED'}}
 $d=Dsh-Observe;$c=Claude-Observe;if(-not $d.ok -or -not $c.ok){throw 'OBSERVATION_UNAVAILABLE'}
 if($d.user_seq -ne $script:r.dsh_user_seq -or $c.latest_user_index -ne $script:r.claude_user_index -or $c.latest_user_hash -cne $script:r.claude_user_hash){throw 'MANUAL_INTERVENTION_DETECTED'}
 if($script:r.direction -eq 'DSH_TO_CLAUDE'){
  if($c.state -in @('running','awaiting_reply','needs_input') -or -not $c.tail_present){throw 'CLAUDE_NOT_IDLE'}
  if($null -eq $d.result -or $d.result.hash -cne $script:r.source_hash -or $d.result.end_seq -ne $script:r.source_seq){throw 'DSH_RESULT_CHANGED'}
 }else{
  if($d.busy -or $d.turn -ne $script:r.dsh_turn){throw 'DSH_NOT_IDLE'}
  if(-not $c.reply_available -or (Hash-Message $c.reply_text) -cne $script:r.source_hash){throw 'CLAUDE_REPLY_CHANGED'}
 }
 return @{Dsh=$d;Claude=$c}
}
