# Reconcile UIA readings without treating an empty DSH input's hint as a draft.
function Resolve-ComposerReadback([bool]$ValueReadable,[string]$ValueText,[bool]$TextReadable,[string]$TextText,[bool]$IsDshInput,[string]$InputHint) {
 if(-not $ValueReadable -and -not $TextReadable){throw 'DRAFT_UNREADABLE'}
 $value=Normalize-Message $ValueText;$text=Normalize-Message $TextText
 $hint=Normalize-Message $InputHint
 if($IsDshInput -and $ValueReadable -and -not $value -and $TextReadable -and $hint -and $text -ceq $hint){return ''}
 if($ValueReadable -and $TextReadable -and $value -and $text -and $value -cne $text){throw 'DRAFT_READBACK_CONFLICT'}
 if($ValueReadable -and $value){return $value}
 if($TextReadable -and $text){return $text}
 return ''
}
