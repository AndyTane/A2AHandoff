# Regression guard for the Claude anchor hash.
#
# Background: auto handoff kept pausing itself right after a successful delivery.
# The cause was the anchor hash. `observe.ps1` hashed the raw text range of the
# latest user message, and that range also contains the message's own view chrome:
# the "Message collapsed" indicator, the "Show more" control and the action-bar
# label. Those change as Claude Desktop re-renders the message, so the anchor hash
# moved while the content was byte-identical. `manual_changed()` saw a different
# hash, concluded a human had intervened, and set phase `paused_by_user` - killing
# the automatic listener after every exchange.
#
# The fix is that the anchor hash now comes from the BOUNDED body range, which is
# cut at the earliest chrome element. This test locks in both halves of that
# contract:
#   * the chrome predicate matches a standalone chrome line and nothing else,
#     so real content that happens to contain the same words is never stripped;
#   * stripping those standalone lines from the full text reproduces the body
#     exactly - that is the invariant `Read-MessageBody` implements.
# It needs no live Claude window.
[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

. (Join-Path $PSScriptRoot '..\adapters\claude\user-body.ps1')
. (Join-Path $PSScriptRoot '..\adapters\claude\message-correlation.ps1')

$n = 0
function Assert-Case($condition, $label) {
    if (-not $condition) { throw $label }
    $script:n++
}

# --- the chrome predicate is line-exact ------------------------------------
Assert-Case (Test-MessageChromeText 'Message collapsed') 'bare collapse indicator is chrome'
Assert-Case (Test-MessageChromeText 'Message expanded') 'bare expand indicator is chrome'
Assert-Case (Test-MessageChromeText '  Message collapsed  ') 'padded indicator is chrome'
Assert-Case (-not (Test-MessageChromeText 'the Message collapsed banner')) 'embedded phrase is content'
Assert-Case (-not (Test-MessageChromeText 'Message collapsed because the panel closed')) 'prefix phrase is content'
Assert-Case (-not (Test-MessageChromeText '')) 'empty is not chrome'
Assert-Case (-not (Test-MessageChromeText $null)) 'null is not chrome'

Assert-Case (Test-MessageChromeButton 'Show more') 'bare Show more is chrome'
Assert-Case (Test-MessageChromeButton 'Show less') 'bare Show less is chrome'
Assert-Case (-not (Test-MessageChromeButton 'Show more options for this message')) 'longer button label is content'
Assert-Case (Test-MessageChromeBar 'Message actions') 'action bar is chrome'
Assert-Case (Test-MessageChromeBar 'Show message actions for You said: hi') 'action bar label is chrome'
Assert-Case (-not (Test-MessageChromeBar 'Show more')) 'button label is not a bar'

# --- the body invariant: strip standalone chrome lines, keep everything else ---
# Mirrors Read-MessageBody's observable effect, and is also used as a live probe.
function Get-ChromeStrippedText([string]$Text) {
    $kept = @($Text -split '\r?\n' | Where-Object {
            -not (Test-MessageChromeText $_) -and
            -not (Test-MessageChromeButton $_) -and
            -not (Test-MessageChromeBar $_)
        })
    return (($kept -join "`n") -replace '\r\n?', "`n" -replace '\s+', ' ').Trim()
}

$content = "DSH 已回复:`n" + ("正文第一段。" * 40) + "`n正文最后一段。"
$withChrome = $content + "`nMessage collapsed`nShow more`nShow message actions for You said: DSH 已回复:"
$expected = Get-ChromeStrippedText $content

Assert-Case ((Get-ChromeStrippedText $withChrome) -ceq $expected) 'chrome lines are removed without touching content'
Assert-Case ((Get-PlainMessageHash (Get-ChromeStrippedText $withChrome)) -ceq (Get-PlainMessageHash $expected)) 'anchor hash is unchanged by collapse chrome'
Assert-Case ((Get-PlainMessageHash (Get-ChromeStrippedText $withChrome)) -cne (Get-PlainMessageHash (($withChrome -replace '\r\n?', "`n" -replace '\s+', ' ').Trim()))) 'raw-range hash DOES move with chrome (the old bug)'

# The body is cut at BOTH ends: it starts after the "You said:" accessibility label
# and ends before the chrome. So the live invariant is that the body survives as a
# contiguous run of the raw text - not that it equals a chrome-stripped raw text,
# which would still carry the label prefix and the action-bar tail.
$rawNormalized = ($withChrome -replace '\r\n?', "`n" -replace '\s+', ' ').Trim()
Assert-Case ($rawNormalized.Contains($expected)) 'body survives as a contiguous run of the raw range'

# Content that merely mentions the chrome words must survive untouched.
$mentions = "请解释 Message collapsed 是什么意思`n以及 Show more 的触发条件"
# Compare against the text as the hash rule sees it: whitespace is collapsed by
# Get-PlainMessageHash, so the fixture can only be compared post-normalization.
$mentionsNormalized = ($mentions -replace '\r\n?', "`n" -replace '\s+', ' ').Trim()
Assert-Case ((Get-ChromeStrippedText $mentions) -ceq $mentionsNormalized) 'content mentioning chrome words is preserved'
Assert-Case ((Get-ChromeStrippedText $mentions).Contains('Message collapsed')) 'literal mention survives'
Assert-Case ((Get-ChromeStrippedText $mentions).Contains('Show more')) 'literal mention survives (2)'

# A real edit must still change the anchor: that is the guard's whole purpose.
$edited = $withChrome.Replace('正文最后一段。', '正文最后一段（已改）。')
Assert-Case ((Get-PlainMessageHash (Get-ChromeStrippedText $edited)) -cne (Get-PlainMessageHash $expected)) 'a real content edit still moves the anchor'

# --- the wiring: the anchor must come from the bounded body ----------------
$observe = [IO.File]::ReadAllText((Join-Path $PSScriptRoot '..\adapters\claude\observe.ps1'))
Assert-Case ($observe -match '\$humanBody=Read-UserMessageBody') 'observe.ps1 reads the bounded user body'
Assert-Case ($observe -match '(?m)^\s*\$humanHash=\$bodyHash\s*$') 'observe.ps1 anchors latest_user_hash on the bounded body hash'
Assert-Case ($observe -match 'latest_user_hash=\$humanHash') 'observe.ps1 still publishes the key the runtime compares'

# The runtime compares `latest_user_hash` against the stored anchor, so no other
# adapter may invent its own user-text hash for that comparison.
$draftContext = [IO.File]::ReadAllText((Join-Path $PSScriptRoot '..\adapters\windows\draft-context.ps1'))
Assert-Case ($draftContext -match 'latest_user_hash -cne \$script:r\.claude_user_hash') 'freshness check compares latest_user_hash against the anchor'

# --- syntax of the files this test depends on ------------------------------
$tokens = $null; $errors = $null
foreach ($path in @('..\adapters\claude\user-body.ps1', '..\adapters\claude\observe.ps1', '..\adapters\windows\draft-context.ps1')) {
    $errors = $null
    [void][System.Management.Automation.Language.Parser]::ParseInput(
        [IO.File]::ReadAllText((Join-Path $PSScriptRoot $path)), [ref]$tokens, [ref]$errors)
    Assert-Case ($errors.Count -eq 0) ('syntax: ' + $path)
}

[pscustomobject]@{ tests_passed = $n; messages_sent = 0 } | ConvertTo-Json -Compress
