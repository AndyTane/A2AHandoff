# Read only within assistant response containers; never fall back to the whole chat.
function Read-ResponseBody($message,$textPattern){
 $roots=[Collections.Generic.List[object]]::new();$scan=[Collections.Generic.Stack[object]]::new();$scan.Push($message);$visited=0
 $ui=@([Windows.Automation.ControlType]::Button,[Windows.Automation.ControlType]::ToolBar,[Windows.Automation.ControlType]::StatusBar)
 $prose=@([Windows.Automation.ControlType]::Group,[Windows.Automation.ControlType]::Text,[Windows.Automation.ControlType]::List,[Windows.Automation.ControlType]::ListItem,[Windows.Automation.ControlType]::Hyperlink,[Windows.Automation.ControlType]::Table,[Windows.Automation.ControlType]::DataItem,[Windows.Automation.ControlType]::Header,[Windows.Automation.ControlType]::HeaderItem)
 while($scan.Count){
  $node=$scan.Pop();$info=$node.Current;$visited++
  if($visited -gt 6000){throw 'REPLY_TREE_TOO_LARGE'}
  if($info.ControlType -in $ui -or $info.ClassName -match '(^|\s)sr-only(\s|$)'){continue}
  if($info.ClassName -match '(^|\s)font-claude-response(\s|$)'){$roots.Add($node);continue}
  $children=[Collections.Generic.List[object]]::new();$child=$walk.GetFirstChild($node)
  while($child){$children.Add($child);$child=$walk.GetNextSibling($child)}
  for($i=$children.Count-1;$i -ge 0;$i--){$scan.Push($children[$i])}
 }
 $parts=[Collections.Generic.List[string]]::new()
 foreach($root in $roots){
  # Plain paragraphs/lists often have no markdown CSS classes. Read their common
  # container once, preserving list numbers and inline text without duplication.
  $safe=$true;$scan.Push($root)
  while($scan.Count){
   $node=$scan.Pop();$info=$node.Current;$visited++
   if($visited -gt 6000){throw 'REPLY_TREE_TOO_LARGE'}
   if($info.ControlType -notin $prose -or $info.ClassName -match '(^|\s)sr-only(\s|$)|(^|\s)(thinking|tool-call|tool-result)(\s|$)'){$safe=$false}
   $child=$walk.GetFirstChild($node);while($child){$scan.Push($child);$child=$walk.GetNextSibling($child)}
  }
  if($safe){$value=$textPattern.RangeFromChild($root).GetText(-1);if($value.Trim()){$parts.Add($value.Trim())};continue}
  # Rich replies retain the existing explicit markdown boundary. UI controls,
  # hidden accessibility summaries and unrelated tool panels are not fallback text.
  $scan.Push($root)
  while($scan.Count){
   $node=$scan.Pop();$info=$node.Current;$visited++
   if($visited -gt 6000){throw 'REPLY_TREE_TOO_LARGE'}
   if($info.ControlType -in $ui -or $info.ClassName -match '(^|\s)sr-only(\s|$)'){continue}
   $body=($info.ClassName -match '(^|\s)row-start-1(\s|$)' -and $info.ClassName -match '(^|\s)col-start-1(\s|$)' -and $info.ClassName -match 'z-\[2\]') -or $info.ClassName -match '(^|\s)(standard-markdown|progressive-markdown)(\s|$)'
   if($body){$value=$textPattern.RangeFromChild($node).GetText(-1);if($value.Trim()){$parts.Add($value.Trim())};continue}
   $children=[Collections.Generic.List[object]]::new();$child=$walk.GetFirstChild($node)
   while($child){$children.Add($child);$child=$walk.GetNextSibling($child)}
   for($i=$children.Count-1;$i -ge 0;$i--){$scan.Push($children[$i])}
  }
 }
 return ($parts -join "`n`n")
}
