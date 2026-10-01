# Receipt for a native Send action on an exact, verified draft.
# This is local UI acceptance evidence, not a claim about cloud task completion.
function Test-NativeSubmitReceipt($Observation,$Request,[bool]$InvokeCompleted,[bool]$ComposerEmpty){
 if(-not $InvokeCompleted -or -not $ComposerEmpty -or -not $Observation.ok){return $false}
 if($Observation.session_id -cne $Request.claude_session){return $false}
 if($Observation.latest_user_index -ne ($Request.claude_reply_index+1)){return $false}
 if($Observation.latest_user_index -le $Request.claude_user_index){return $false}
 $hash=$Observation.PSObject.Properties['latest_user_hash']
 return ($null -ne $hash -and -not [string]::IsNullOrWhiteSpace([string]$hash.Value))
}
