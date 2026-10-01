# Local-only, full-content correlation. No message markers or partial-prefix matches.
function Get-PlainMessageHash([AllowEmptyString()][string]$Text) {
 $normalized=($Text -replace '\s+',' ').Trim()
 $sha=[Security.Cryptography.SHA256]::Create()
 try { return ([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($normalized)))).Replace('-','').ToLowerInvariant() }
 finally { $sha.Dispose() }
}
function Test-PlainClaudeReceipt($Observation,$Request,[string]$ExpectedText) {
 if(-not $Observation.ok){return $false}
 if($Observation.session_id -cne $Request.claude_session){return $false}
 if($Observation.latest_user_index -ne ($Request.claude_reply_index+1)){return $false}
 $property=$Observation.PSObject.Properties['latest_user_body_hash']
 if($null -eq $property -or [string]::IsNullOrWhiteSpace($ExpectedText)){return $false}
 return ([string]$property.Value -ceq (Get-PlainMessageHash $ExpectedText))
}
