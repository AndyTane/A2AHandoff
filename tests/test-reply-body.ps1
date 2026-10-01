Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
. (Join-Path $PSScriptRoot '../adapters/claude/reply-body.ps1')
$types=@{Group=[Windows.Automation.ControlType]::Group;Text=[Windows.Automation.ControlType]::Text;Button=[Windows.Automation.ControlType]::Button;ToolBar=[Windows.Automation.ControlType]::ToolBar;List=[Windows.Automation.ControlType]::List;ListItem=[Windows.Automation.ControlType]::ListItem;Pane=[Windows.Automation.ControlType]::Pane;Hyperlink=[Windows.Automation.ControlType]::Hyperlink}
function Node($type,$class,$value,[object[]]$children=@()){
 $n=[pscustomobject]@{Current=[pscustomobject]@{ControlType=$types[$type];ClassName=$class};Value=$value;Children=@($children);Parent=$null;Index=0}
 for($i=0;$i -lt $children.Count;$i++){$children[$i].Parent=$n;$children[$i].Index=$i}
 $n|Add-Member ScriptMethod GetText {param($limit) return $this.Value}
 return $n
}
$walk=[pscustomobject]@{}
$walk|Add-Member ScriptMethod GetFirstChild {param($n) if($n.Children.Count){return $n.Children[0]};return $null}
$walk|Add-Member ScriptMethod GetNextSibling {param($n) if($null -ne $n.Parent -and $n.Index+1 -lt $n.Parent.Children.Count){return $n.Parent.Children[$n.Index+1]};return $null}
$pattern=[pscustomobject]@{}
$pattern|Add-Member ScriptMethod RangeFromChild {param($n) return $n}
$passed=[Collections.Generic.List[string]]::new()
function Check($name,$node,$expected){$actual=Read-ResponseBody $node $pattern;if($actual -cne $expected){throw ('FAILED '+$name+': '+$actual)};$passed.Add($name)}
$body=Node 'Group' 'font-claude-response' 'Short reply' @((Node 'Group' '' 'Short reply' @((Node 'Text' '' 'Short reply'))))
$msg=Node 'Group' '' 'WRONG whole message text' @((Node 'Text' 'sr-only select-none' 'Claude responded: Short reply'),$body,(Node 'ToolBar' '' 'Copy Good response 5 minutes ago'))
Check 'unstyled_short_reply_excludes_duplicate_summary_and_toolbar' $msg 'Short reply'
$list="First paragraph`n1. one`n2. two`nClosing paragraph"
$body=Node 'Group' 'font-claude-response relative' $list @((Node 'Group' '' 'First paragraph'),(Node 'List' '' '1. one 2. two' @((Node 'ListItem' '' 'one'),(Node 'ListItem' '' 'two'))),(Node 'Group' '' 'Closing paragraph'))
Check 'paragraphs_and_numbered_list' $body $list
$body=Node 'Group' 'font-claude-response' 'Copy and Read aloud are literal words here' @((Node 'Text' '' 'Copy and Read aloud are literal words here'))
Check 'literal_ui_words_are_preserved_in_prose' $body 'Copy and Read aloud are literal words here'
$md=Node 'Group' 'standard-markdown' "Header`ncode {x}`nend" @((Node 'Text' '' 'Header code end'))
$body=Node 'Group' 'font-claude-response' 'DO NOT READ UI WITH BODY' @((Node 'Button' '' 'Thinking'),$md)
Check 'rich_standard_markdown_excludes_other_buttons' $body "Header`ncode {x}`nend"
$inner=Node 'Group' 'progressive-markdown' 'Nested text'
$body=Node 'Group' 'font-claude-response' 'Nested text' @((Node 'Group' 'standard-markdown' 'Nested text' @($inner)))
Check 'nested_markdown_is_not_duplicated' $body 'Nested text'
$body=Node 'Group' 'font-claude-response' 'SECRET TOOL PANEL' @((Node 'Pane' '' 'SECRET TOOL PANEL'),(Node 'Button' '' 'Run'))
Check 'unknown_non_text_layout_does_not_leak_whole_message' $body ''
$body=Node 'Group' 'font-claude-response' 'HIDDEN SUMMARY' @((Node 'Text' 'sr-only' 'HIDDEN SUMMARY'))
Check 'hidden_only_response_is_not_used' $body ''
$body=Node 'Group' 'font-claude-response' 'DO NOT READ UI WITH BODY' @((Node 'Button' '' 'Copy'),(Node 'Group' 'row-start-1 col-start-1 z-[2]' 'Layered markdown'))
Check 'legacy_layered_markdown' $body 'Layered markdown'
$body=Node 'Group' 'font-claude-response' 'DO NOT READ UI WITH BODY' @((Node 'Button' '' 'Copy'),(Node 'Group' 'progressive-markdown' 'Progressive markdown'))
Check 'legacy_progressive_markdown' $body 'Progressive markdown'
$msg=Node 'Group' '' 'Full conversation' @((Node 'Group' 'font-claude-response' 'First'),(Node 'Group' 'font-claude-response' 'Second'))
Check 'multiple_response_blocks_keep_order' $msg "First`n`nSecond"
Check 'no_response_container_is_not_old_message_fallback' (Node 'Group' 'other-font-claude-response' 'Unrelated') ''
Check 'empty_prose_returns_empty' (Node 'Group' 'font-claude-response' '   ') ''
$nodes=@();for($i=0;$i -lt 6100;$i++){$nodes+=@(Node 'Text' '' 'x')}
$huge=Node 'Group' 'font-claude-response' 'huge' $nodes
$blocked=$false;try{$null=Read-ResponseBody $huge $pattern}catch{if($_.Exception.Message -eq 'REPLY_TREE_TOO_LARGE'){$blocked=$true}else{throw}}
if(-not $blocked){throw 'Tree bound failed'};$passed.Add('oversize_tree_fails_closed')
[pscustomobject]@{tests_passed=$passed.Count;names=$passed;messages_sent=0}|ConvertTo-Json -Depth 4 -Compress
