# Read-only UIA helpers for staged sending.
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
function Get-Field($Object,$Name,$Default=$null) {
 if($null -ne $Object -and $null -ne $Object.PSObject.Properties[$Name]){return $Object.$Name};return $Default
}
function Normalize-Message([string]$Text){
 # Line endings and per-line trailing whitespace are noise: both target editors re-encode
 # them, and one extra CR used to fail the exact-match check that guards a send, with a bare
 # DRAFT_WRITE_UNVERIFIED and nothing to look at. Every visible character is still compared
 # - this only stops invisible re-encoding from blocking a delivery.
 $t=$Text.Replace("`r`n","`n").Replace("`r","`n")
 $t=($t -split "`n" | ForEach-Object { $_.TrimEnd() }) -join "`n"
 return $t.Replace([string][char]0xFEFF,'').Replace([string][char]0x200B,'').Trim()
}
# Where do two drafts differ? "Not verified" on its own leaves nothing to inspect, which is
# what made DRAFT_WRITE_UNVERIFIED impossible to diagnose. This reports both sizes and the
# first divergence with a little context, in a form that survives a log line.
function Compare-DraftText([string]$Written,[string]$ReadBack){
 $w=Normalize-Message $Written;$r=Normalize-Message $ReadBack
 $n=[Math]::Min($w.Length,$r.Length);$i=0
 while($i -lt $n -and $w[$i] -ceq $r[$i]){$i++}
 $from=[Math]::Max(0,$i-20)
 $slice={param($s) $len=[Math]::Min(48,$s.Length-$from);if($len -le 0){return ''};return ($s.Substring($from,$len) -replace "`n",'\n')}
 return ('written={0} read={1} first_diff={2} written_ctx=[{3}] read_ctx=[{4}]' -f $w.Length,$r.Length,$i,(& $slice $w),(& $slice $r))
}
function Hash-Message([string]$Text) {
 $sha=[Security.Cryptography.SHA256]::Create()
 try{return ([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes((Normalize-Message $Text))))).Replace('-','').ToLowerInvariant()}finally{$sha.Dispose()}
}
function Get-UiCondition($Type) {return [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty,$Type)}
function Get-DocHash($Document) {
 $vp=$null
 if($Document.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern,[ref]$vp)){return Hash-Message ([string]$vp.Current.Value)}
 return ''
}
