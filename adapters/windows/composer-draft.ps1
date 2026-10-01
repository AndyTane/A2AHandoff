# Plain-text drafts only. IDs and receipts stay local. No confirmation UI.
. (Join-Path $PSScriptRoot 'composer-readback.ps1')
function Read-ComposerText($M) {
 $valueReadable=$false;$textReadable=$false;$value='';$text=''
 try{$value=[string]$M.Value.Current.Value;$valueReadable=$true}catch{}
 try{$tp=$null;if($M.Edit.TryGetCurrentPattern([Windows.Automation.TextPattern]::Pattern,[ref]$tp)){$text=[string]$tp.DocumentRange.GetText(-1);$textReadable=$true}}catch{}
 $dsh=($M.Document.Current.Name -like ('*'+$M.TitlePattern+'*')) -and $M.Edit.Current.ClassName -match '(^|_)input$'
 return (Resolve-ComposerReadback $valueReadable $value $textReadable $text $dsh ([string]$M.Edit.Current.Name))
}
function Assert-ComposerIdentity($M) {
 if($M.Document.Current.Name -cne $M.Title -or (Get-DocHash $M.Document) -cne $M.DocumentHash){throw 'TARGET_CHANGED'}
 if(-not $M.Edit.Current.IsEnabled -or $M.Edit.Current.IsOffscreen){throw 'INPUT_NOT_AVAILABLE'}
}
function Get-ComposerSendButton($M) {
 $ae=[Windows.Automation.AutomationElement]
 $names=[Windows.Automation.Condition[]]@('Send','Send message','发送','发送消息'|ForEach-Object {[Windows.Automation.PropertyCondition]::new($ae::NameProperty,$_ )})
 $nameCondition=[Windows.Automation.OrCondition]::new($names)
 $condition=[Windows.Automation.AndCondition]::new([Windows.Automation.Condition[]]@(
  (Get-UiCondition ([Windows.Automation.ControlType]::Button)),
  [Windows.Automation.PropertyCondition]::new($ae::IsOffscreenProperty,$false),
  [Windows.Automation.PropertyCondition]::new($ae::IsEnabledProperty,$true),$nameCondition))
 $buttons=$M.Document.FindAll([Windows.Automation.TreeScope]::Descendants,$condition)
 $unique=@{};foreach($b in $buttons){$unique[($b.GetRuntimeId() -join ':')]=$b}
 if($unique.Count -gt 1){throw 'SEND_BUTTON_AMBIGUOUS'}
 if($unique.Count -eq 1){return @($unique.Values)[0]}
 return $null
}
function Test-ExactDraft($M,[string]$Expected){return ((Read-ComposerText $M) -ceq (Normalize-Message $Expected))}
function Set-PlainOwnedDraft($M,[string]$Expected,[scriptblock]$BeforeWrite) {
 Assert-ComposerIdentity $M
 if(Read-ComposerText $M){throw 'DRAFT_OCCUPIED_PRESERVED'}
 # A sendable attachment in an otherwise empty editor is also user content.
 if($null -ne (Get-ComposerSendButton $M)){throw 'EXISTING_ATTACHMENT_PRESERVED'}
 if($M.Value.Current.IsReadOnly){throw 'PLAIN_TEXT_INPUT_UNSUPPORTED'}
 if(($M.Document.Current.Name -like ('*'+$M.TitlePattern+'*')) -and $M.Edit.Current.ClassName -match '(^|_)input$'){
  . (Join-Path $PSScriptRoot 'dsh-keyboard-input.ps1')
  $null=Set-ComposerDraft $M $Expected $BeforeWrite
  if(-not (Test-ExactDraft $M $Expected)){throw 'DRAFT_WRITE_UNVERIFIED'}
  return
 }
 & $BeforeWrite
 Assert-ComposerIdentity $M
 if(Read-ComposerText $M){throw 'DRAFT_CHANGED_BEFORE_WRITE'}
 $M.Value.SetValue($Expected)
 for($i=0;$i -lt 30;$i++){
  Start-Sleep -Milliseconds 100;Assert-ComposerIdentity $M
  if(Test-ExactDraft $M $Expected){return}
 }
 throw 'DRAFT_WRITE_UNVERIFIED'
}
function Submit-VerifiedDraft($M,[string]$Expected,[scriptblock]$BeforeSend) {
 Assert-ComposerIdentity $M
 if(-not (Test-ExactDraft $M $Expected)){throw 'DRAFT_EDITED_SEND_CANCELLED'}
 $button=Get-ComposerSendButton $M
 if($null -eq $button){throw 'SEND_BUTTON_NOT_READY'}
 $invoke=$null
 if(-not $button.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern,[ref]$invoke)){throw 'SEND_BUTTON_INVOKE_UNSUPPORTED'}
 & $BeforeSend
 Assert-ComposerIdentity $M
 if(-not (Test-ExactDraft $M $Expected)){throw 'DRAFT_EDITED_SEND_CANCELLED'}
 $invoke.Invoke() # Exactly one native send action. Never repeat on an uncertain receipt.
}
