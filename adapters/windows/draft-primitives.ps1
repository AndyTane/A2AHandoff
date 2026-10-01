# Read-only UIA helpers for staged sending.
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
function Get-Field($Object,$Name,$Default=$null) {
 if($null -ne $Object -and $null -ne $Object.PSObject.Properties[$Name]){return $Object.$Name};return $Default
}
function Normalize-Message([string]$Text){return ($Text.Replace("`r`n","`n").Replace("`r","`n").Trim())}
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
