# DSH-only focus and one plain-text paste. No Enter or confirmation code.
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'

[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Windows.Forms,Microsoft.VisualBasic
if(-not ('HandoffNativeV2' -as [type])) {
 Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class HandoffNativeV2 {
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern short GetAsyncKeyState(int k);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
 [DllImport("user32.dll")] public static extern bool ShowWindowAsync(IntPtr h, int command);
}
'@
}
function Focus-Composer($M) {
 # Focus acquisition may lag for background workers. Retry focus only, never keys.
 for($attempt=0;$attempt -lt 4;$attempt++){
  foreach($key in @(16,17,18)){if(([HandoffNativeV2]::GetAsyncKeyState($key) -band 0x8000) -ne 0){throw 'Release Shift/Ctrl/Alt before sending.'}}
  if($M.Document.Current.Name -cne $M.Title -or (Get-DocHash $M.Document) -cne $M.DocumentHash){throw 'Target changed. No key was sent.'}
  if([HandoffNativeV2]::IsIconic($M.Window.MainWindowHandle)){[void][HandoffNativeV2]::ShowWindowAsync($M.Window.MainWindowHandle,9)}
  [Microsoft.VisualBasic.Interaction]::AppActivate($M.Window.Id)
  [void][HandoffNativeV2]::SetForegroundWindow($M.Window.MainWindowHandle)
  Start-Sleep -Milliseconds 250
  $M.Edit.SetFocus();Start-Sleep -Milliseconds 200
  if([HandoffNativeV2]::GetForegroundWindow() -eq $M.Window.MainWindowHandle -and $M.Edit.Current.HasKeyboardFocus -and $M.Document.Current.Name -ceq $M.Title -and (Get-DocHash $M.Document) -ceq $M.DocumentHash){return}
 }
 throw 'FOCUS_PENDING: target input could not be focused. No key was sent.'
}
function Set-ComposerDraft($M,[string]$Text,[scriptblock]$BeforePaste=$null) {
 $desired=Normalize-Message $Text;$existing=Normalize-Message $M.Value.Current.Value
 if($existing -ceq $desired){return 'already_present'}
 if($existing){throw 'DRAFT_OCCUPIED: existing draft was preserved.'}
 Focus-Composer $M
 if((Normalize-Message $M.Value.Current.Value)){throw 'Draft changed. Nothing pasted.'}
 $old=[Windows.Forms.Clipboard]::GetDataObject()
 try{
  [Windows.Forms.Clipboard]::SetText($Text)
  if([HandoffNativeV2]::GetForegroundWindow() -ne $M.Window.MainWindowHandle -or -not $M.Edit.Current.HasKeyboardFocus -or $M.Document.Current.Name -cne $M.Title -or (Get-DocHash $M.Document) -cne $M.DocumentHash){throw 'FOCUS_PENDING: focus changed before paste. No key was sent.'}
  if($null -ne $BeforePaste){& $BeforePaste}
  [Windows.Forms.SendKeys]::SendWait('^v')
  for($i=0;$i -lt 30;$i++){Start-Sleep -Milliseconds 100;if((Normalize-Message $M.Value.Current.Value) -ceq $desired){return 'pasted'}}
  throw 'PASTE_UNCERTAIN: no automatic retry; inspect the destination input.'
 }finally{
  try{if([Windows.Forms.Clipboard]::ContainsText() -and [Windows.Forms.Clipboard]::GetText() -ceq $Text){if($null -ne $old){[Windows.Forms.Clipboard]::SetDataObject($old,$true)}else{[Windows.Forms.Clipboard]::Clear()}}}catch{}
 }
}
