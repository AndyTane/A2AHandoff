# Guards the draft-readback normalisation and the diagnostic that accompanies a failure.
#
# The send guard is an exact match between what was written into the target input box and
# what can be read back. Both target editors re-encode line endings and drop trailing
# spaces, and one invisible difference used to block a delivery with a bare
# DRAFT_WRITE_UNVERIFIED - which said nothing about what to look at. These checks pin the
# tolerances (invisible differences only) and the diagnostic.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
. (Join-Path $PSScriptRoot '../adapters/windows/draft-primitives.ps1')

$passed = [Collections.Generic.List[string]]::new()
function Check($name, $actual, $expected) {
    if ($actual -cne $expected) {
        throw ('FAILED ' + $name + ': got [' + $actual + '] want [' + $expected + ']')
    }
    $passed.Add($name)
}

# The same text, re-encoded by an editor, must compare equal.
Check 'crlf_and_cr_become_lf' (Normalize-Message "a`r`nb`rc") "a`nb`nc"
Check 'trailing_spaces_on_lines_are_noise' (Normalize-Message "a   `nb`t`n") "a`nb"
Check 'outer_blank_lines_are_trimmed' (Normalize-Message "`n`na`n`n") 'a'
Check 'bom_and_zero_width_are_dropped' (Normalize-Message ([string][char]0xFEFF + 'a' + [string][char]0x200B + 'b')) 'ab'

# Real differences must still block a send: the guard is not weakened.
Check 'inner_spaces_still_matter' ((Normalize-Message 'a  b') -ceq (Normalize-Message 'a b')) $false
Check 'case_still_matters' ((Normalize-Message 'Ab') -ceq (Normalize-Message 'ab')) $false
Check 'a_missing_line_still_matters' ((Normalize-Message "a`nb") -ceq (Normalize-Message 'a b')) $false

# This digest is shared by THREE implementations that are compared against each other:
#   crates/handoff-runtime/src/main.rs        hash()            (live_tests::text_normalisation_matches_the_adapters)
#   adapters/windows/draft-primitives.ps1     Normalize-Message (the check below)
#   adapters/dsh/observe.mjs                  hash              (the node check below)
# A drift in any one of them is read as "the message changed" - DSH_RESULT_CHANGED looping
# forever, or submit_uncertain recorded for a message that did arrive. That is exactly what
# happened on 2026-10-01, twice: first the runtime's copy was missed, then observe.mjs's.
$sharedDigest = '515d303ff7d38d7ea89acc4a4bfa65dca39d7b0c65cd369da110781ba875d959'
$shared = "A  `r`nB" + [string][char]0xFEFF + [string][char]0x200B + "`r`n   `r`nC  "
Check 'the_normalisation_rule_is_shared_with_the_runtime' (Hash-Message $shared) $sharedDigest

# The JavaScript copy. Importing the module has no side effects: its body only runs when it is
# invoked directly, so this hashes the fixture without touching any session.
$root = Split-Path $PSScriptRoot -Parent
$observer = (Join-Path $root 'adapters/dsh/observe.mjs').Replace('\', '/')
$js = "import('file:///$observer').then(m=>console.log(m.hash('A  \r\nB\uFEFF\u200B\r\n   \r\nC  ')))"
if (Get-Command node.exe -ErrorAction SilentlyContinue) {
    $observed = (& node.exe -e $js 2>&1 | Select-Object -Last 1)
    Check 'the_normalisation_rule_is_shared_with_the_dsh_observer' ([string]$observed).Trim() $sharedDigest
} else {
    Write-Output 'WARNING: node.exe not found, so the DSH observer hash was not checked.'
}

# The diagnostic reports both sizes and the first divergence, with context.
$same = Compare-DraftText "line`nline" "line`r`nline"
Check 'identical_after_normalisation_reports_lengths' ($same -match '^written=9 read=9 first_diff=9 ') $true
$diff = Compare-DraftText 'hello world' 'hello WORLD'
Check 'first_difference_index' ($diff -match 'first_diff=6 ') $true
Check 'context_shows_both_sides' (($diff -match 'written_ctx=\[hello world\]') -and ($diff -match 'read_ctx=\[hello WORLD\]')) $true
$short = Compare-DraftText 'abc' 'abcdef'
Check 'a_prefix_difference_is_found' ($short -match 'written=3 read=6 first_diff=3 ') $true

Write-Output ("draft readback: {0} passed, 0 failed" -f $passed.Count)
Write-Output 'No messages were sent.'
