# Read-only delayed receipt recovery. Never writes a draft and never invokes Send.
param(
 [Parameter(Mandatory=$true)][string]$ProductRoot,
 [Parameter(Mandatory=$true)][string]$RequestFile
)
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
. "$PSScriptRoot\draft-context.ps1"
. "$PSScriptRoot\composer-draft.ps1"
. (Join-Path $PSScriptRoot '..\claude\submit-evidence.ps1')
function Emit($v){[Console]::WriteLine(($v|ConvertTo-Json -Depth 8 -Compress))}
try{
 $script:r=Read-V1 $RequestFile
 $script:b=Read-V1 (Join-Path $ProductRoot 'runtime/bindings.json')
 if($script:r.direction -cne 'DSH_TO_CLAUDE'){throw 'RECOVERY_DIRECTION_UNSUPPORTED'}
 $rp=Join-Path $ProductRoot ('runtime/receipts/'+$script:r.id+'.json')
 if(-not (Test-Path -LiteralPath $rp)){throw 'RECOVERY_RECEIPT_MISSING'}
 $receipt=Read-V1 $rp
 if($receipt.state -cne 'submit_uncertain'){throw 'RECOVERY_RECEIPT_NOT_UNCERTAIN'}
 if($receipt.outgoing_hash -cne (Hash-Message $script:r.text)){throw 'RECOVERY_REQUEST_CHANGED'}
 $after=Claude-Observe
 if(-not $after.ok){throw 'RECOVERY_OBSERVATION_UNAVAILABLE'}
 $m=Bound-Composer $false $after
 $empty=[string]::IsNullOrWhiteSpace((Read-ComposerText $m))
 if(-not (Test-NativeSubmitReceipt $after $script:r $true $empty)){throw 'RECOVERY_EVIDENCE_INSUFFICIENT'}
 $anchor=[ordered]@{
  claude_user_index=$after.latest_user_index
  claude_user_hash=$after.latest_user_hash
  evidence='delayed_native_submit_next_user_empty_composer'
 }
 Emit ([ordered]@{ok=$true;state='sent';id=$script:r.id;anchor=$anchor;messages_sent=0})
}catch{
 Emit ([ordered]@{ok=$false;state='hold';error=$_.Exception.Message;messages_sent=0})
 exit 1
}
