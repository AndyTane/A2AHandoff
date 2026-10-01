$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot '..\adapters\claude\message-correlation.ps1')
$n=0
function Assert-Case($condition,$label){if(-not $condition){throw $label};$script:n++}
$r=[pscustomobject]@{claude_session='cse_test';claude_reply_index=8}
$o=[pscustomobject]@{ok=$true;tail_present=$true;session_id='cse_test';latest_user_index=9;latest_user_body_hash=(Get-PlainMessageHash "[complete reply]")}
Assert-Case (Test-PlainClaudeReceipt $o $r '[complete reply]') 'valid next message'
$o.session_id='cse_wrong'
Assert-Case (-not (Test-PlainClaudeReceipt $o $r '[complete reply]')) 'wrong conversation'
$o.session_id='cse_test';$o.latest_user_index=11
Assert-Case (-not (Test-PlainClaudeReceipt $o $r '[complete reply]')) 'intervening user message'
$o.latest_user_index=9;$o.latest_user_body_hash=Get-PlainMessageHash '[complete reply CHANGED]'
Assert-Case (-not (Test-PlainClaudeReceipt $o $r '[complete reply]')) 'same prefix is insufficient'
$o.latest_user_body_hash=Get-PlainMessageHash '[complete reply]';$o.tail_present=$false
Assert-Case (Test-PlainClaudeReceipt $o $r '[complete reply]') 'submitted user remains verifiable during assistant streaming'
$o.tail_present=$true
Assert-Case (-not (Test-PlainClaudeReceipt $o $r '')) 'empty expected text'
Assert-Case ((Get-PlainMessageHash "a`r`nb") -ceq (Get-PlainMessageHash "a b")) 'line wrapping canonicalization'
Assert-Case ((Get-PlainMessageHash 'BODY') -cne (Get-PlainMessageHash 'body')) 'case changes remain meaningful'
$tokens=$null;$errors=$null
foreach($path in @('..\adapters\windows\draft-flow.ps1','..\adapters\claude\observe.ps1')){
 [void][System.Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot $path),[ref]$tokens,[ref]$errors)
 Assert-Case ($errors.Count -eq 0) ('syntax: '+$path)
}
[pscustomobject]@{tests_passed=$n;messages_sent=0}|ConvertTo-Json -Compress
