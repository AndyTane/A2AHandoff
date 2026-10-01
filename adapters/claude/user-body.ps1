# Select only a message body, between its accessibility label and any view chrome.
#
# Why this is boundary arithmetic and not string replacement: the user's own text
# can legitimately contain the same words as the chrome ("Message collapsed",
# "Show more"). Replacing repeated strings would corrupt real content, so every cut
# is made with UIA text ranges.
#
# Why the chrome must be cut at all: `latest_user_hash` is used as the anchor that
# proves "nobody touched this message". The raw node range also contains the
# collapse indicator, the expand/collapse control and the action-bar label. Those
# change as the view re-renders, which made the anchor hash drift while the content
# stayed identical, so the manual-intervention guard fired spuriously and auto
# handoff paused itself after a successful delivery. See design/DELIVERY_NOTES.md.
#
# The chrome predicates are functions, not inline regexes, so tests can assert the
# exact matching rule (standalone line only) without a live Claude window.
function Test-MessageChromeText([string]$Name) {
 if($null -eq $Name){return $false}
 return [bool]($Name -match '^\s*Message (collapsed|expanded)\s*$')
}
function Test-MessageChromeButton([string]$Name) {
 if($null -eq $Name){return $false}
 return [bool]($Name -match '^\s*Show (more|less)\s*$')
}
function Test-MessageChromeBar([string]$Name) {
 if($null -eq $Name){return $false}
 return [bool]($Name -eq 'Message actions' -or $Name -like 'Show message actions for *')
}
function Get-MessageChromeCandidates($node) {
 $all=@($node.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition))
 $found=@()
 $found+=@($all|Where-Object {$_.Current.ControlType -eq [Windows.Automation.ControlType]::Text -and (Test-MessageChromeText $_.Current.Name)})
 $found+=@($all|Where-Object {$_.Current.ControlType -eq [Windows.Automation.ControlType]::Button -and (Test-MessageChromeButton $_.Current.Name)})
 $found+=@($all|Where-Object {$_.Current.ControlType -eq [Windows.Automation.ControlType]::ToolBar -and $_.Current.Name -eq 'Message actions'})
 # The action bar's own accessibility label is exposed as a separate element and
 # reaches past the toolbar range, so it is trimmed too.
 $found+=@($all|Where-Object {$_.Current.Name -like 'Show message actions for *'})
 return ,@($found)
}
function Read-MessageBody($node,$textPattern,$labelPrefix) {
 $range=$textPattern.RangeFromChild($node).Clone()
 $all=@($node.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition))
 $labels=@($all|Where-Object {$_.Current.ClassName -match '(^|\s)sr-only(\s|$)' -and $_.Current.Name -like ($labelPrefix+':*')})
 if($labels.Count -eq 1){
  $label=$textPattern.RangeFromChild($labels[0])
  $range.MoveEndpointByRange([Windows.Automation.Text.TextPatternRangeEndpoint]::Start,$label,[Windows.Automation.Text.TextPatternRangeEndpoint]::End)
 }elseif($labels.Count -gt 1){throw 'MESSAGE_BODY_LABEL_AMBIGUOUS'}
 $chrome=Get-MessageChromeCandidates $node
 if($chrome.Count -eq 0){return $range.GetText(-1)}
 # End the range at the earliest chrome element, whatever its kind, so the body is
 # never cut short by chrome that appears after it and never keeps chrome that
 # appears before it.
 $starts=@()
 foreach($element in $chrome){
  $starts+=[pscustomobject]@{Element=$element;Range=$textPattern.RangeFromChild($element)}
 }
 $cuts=@()
 foreach($entry in $starts){
  try{
   $probe=$range.Clone()
   $probe.MoveEndpointByRange([Windows.Automation.Text.TextPatternRangeEndpoint]::End,$entry.Range,[Windows.Automation.Text.TextPatternRangeEndpoint]::Start)
   $cuts+=[pscustomobject]@{Entry=$entry;Start=$probe.GetText(-1).Length}
  }catch{continue}
 }
 if($cuts.Count -eq 0){return $range.GetText(-1)}
 $earliest=$cuts|Sort-Object Start|Select-Object -First 1
 $range.MoveEndpointByRange([Windows.Automation.Text.TextPatternRangeEndpoint]::End,$earliest.Entry.Range,[Windows.Automation.Text.TextPatternRangeEndpoint]::Start)
 return $range.GetText(-1)
}
# Select only the user message body between its accessibility label and action bar.
function Read-UserMessageBody($node,$textPattern) {
 return Read-MessageBody $node $textPattern 'You said'
}
