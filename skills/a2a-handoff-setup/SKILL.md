---
name: a2a-handoff-setup
description: Install, configure, repair or verify A2AHandoff — the local Windows tool that hands tasks between DeepSeek Harness and Claude Desktop. Use when the user wants to set it up or download it, asks how to bind the Claude cse_… Session ID to a DSH session, or reports that the window shows 未绑定 / 交接已暂缓 / 读取失败 / 点了没反应 / 不知道它在读哪个会话.
---

# Setting up A2AHandoff

A2AHandoff relays work between **DeepSeek Harness (DSH)** and **Claude Desktop** on one Windows
machine. It watches the DSH session and the Claude conversation the user bound, writes the
outgoing draft into the destination input, gives a cancellable delay, then submits once. It is
fail-closed: a changed draft, an ambiguous target, manual intervention or an uncertain submit all
stop automatic retry.

You do the machine's part. The user does the identity part.

## Your boundary

**You may, without asking:** read the product folder, run the read-only checks below, run
`scripts\bootstrap.ps1` (it writes only machine-local `runtime\config.json`), start the app, and
read `runtime\state.json`, `runtime\events.jsonl`, `runtime\receipts\`.

**Ask the user first:** which sessions to bind. The Claude `cse_…` Session ID and the DSH session
say *who is talking to whom*; picking them is the user's decision, and getting it wrong sends a
real task into the wrong conversation.

**Never:** press 发送给 Claude / 发送给 DSH / 重试本次投递 / 取消本次 for the user; write into
`runtime\commands\` to trigger a send; or edit `runtime\receipts\`, `runtime\workflow.json` or
`runtime\events.jsonl`. Those are the record of what actually left the machine, and a handoff is
supposed to be deliberate.

## Find the product folder

The folder holding `A2AHandoff.exe`, `handoff-runtime.exe`, `adapters\`, `scripts\` and
`runtime\`. From a release zip it is the unzipped folder; from source it is the repository root.
Keep it intact — both executables load `adapters\` from beside them, and an `A2AHandoff.exe`
copied out on its own can read and send nothing.

## Set it up

```powershell
# 1. Requirements: Claude Desktop, DSH with its web UI running, Node.js whose zlib has zstd
#    support, Windows PowerShell 5.1, Edge or Chrome.
powershell -ExecutionPolicy Bypass -File .\scripts\check-prereqs.ps1

# 2. Writes runtime\config.json, detects the DSH data directory and the DSH web origin,
#    and copies the message templates. Refuses to overwrite unless given -Force.
powershell -ExecutionPolicy Bypass -File .\scripts\bootstrap.ps1

# 3. Start it.
.\A2AHandoff.exe --product-root .
```

Starting `A2AHandoff.exe` with no argument does the same thing: it finds its own folder, writes
`runtime\config.json` and detects DSH itself. `bootstrap.ps1` is the explicit path and prints
what it detected as JSON — report that to the user rather than paraphrasing it.

If the user has no binary yet, the release zip is the fastest route:
<https://github.com/AndyTane/A2AHandoff/releases/latest>. Building from source is
`cargo build --release --locked` and then the same three steps.

## Bind the two sessions — with the user

Both happen in the window, in **绑定配置**:

- **DSH:** pick the session from the discovered list. The list comes from `dsh_data_home` in
  `runtime\config.json`; an empty list means that path is wrong, and `bootstrap.ps1` is what
  detects it.
- **Claude:** the `cse_…` Session ID. The user can read it from the Cowork conversation's URL
  (`…/cowork/cse_…`) — the adapter matches on host and that path, so the ID is the identity, not
  the window title.

Then the user turns on **监听** (observe) and **自动交接** (automatic handoff). Neither is on in a
fresh install, and nothing is sent while the bindings are the shipped placeholders.

## Check that it worked

```powershell
powershell -ExecutionPolicy Bypass -File .\tests\test-runtime-coherence.ps1
```

Exit 0 means the invariants held (or nothing is running); exit 1 prints what broke. It only
reads — safe to run at any time, including while the app is in use.

Then confirm what the user sees, in the window itself:

- Before binding: both cards read **未绑定**, and the banner reads **还不能开始：会话尚未绑定**
  with a 绑定配置 button. That is the shipped state, not a fault.
- After binding: the Claude card reads **已绑定** with a green 「会话已核验」, and each card's
  当前任务 shows the session name the runtime is actually reading — the conversation name for
  Claude, the DSH session title for DSH. If 当前任务 shows `Claude Desktop` or `–`, the binding
  is still a placeholder or the read is failing.

`runtime\state.json` is what the runtime **publishes**, not what the window **shows**. Never
conclude "the window says X" from `state.json`; ask for a screenshot when the wording matters.

## When the user says it does not work

Read `runtime\events.jsonl` before guessing — one JSON object per line, and `send_refused`,
`delivery_receipt`, `draft_ready` and `manual_intervention` explain most stuck handoffs. Then
`runtime\receipts\<id>.json`: `sent` and `submit_uncertain` are terminal, everything else means
the draft never left.

| What the user reports | What it usually is |
| --- | --- |
| 未绑定 on both cards | Still the shipped placeholders. Bind both sessions in 绑定配置. |
| 交接已暂缓：无法读取 Claude 会话 | The Cowork conversation is not open in Claude Desktop, or the bound `cse_…` is not the one on screen. |
| 交接已暂缓：无法读取 DSH 输出 | DSH is not running, or its web UI is not up. |
| 无法读取 DSH / the session list is empty | `dsh_data_home` is wrong; re-run `scripts\bootstrap.ps1`. |
| Nothing at all after double-clicking | The executable is separated from `adapters\`, or it is a build older than 0.2.1. |
| `DRAFT_OCCUPIED_PRESERVED` | The destination input already had text. The user clears it and presses 重试本次投递. |
| 发送未确认 (`submit_uncertain`) | A send was clicked but delivery could not be confirmed. It is **never** retried automatically — do not retry it either; the user decides. |
| 监听 shows 中断/已停止 | The runtime child died; check `runtime\runtime-stderr.log` and restart the app. |

`runtime\runtime-stderr.log` holds one line per crash. Read it as UTF-8 or it looks like mojibake.

## Tell the user what you did

End by stating: the product folder, what `bootstrap.ps1` detected, which sessions are bound (and
that the user chose them), whether 监听 and 自动交接 are on, and what the window currently shows.
Do not report a handoff as delivered unless a receipt says `sent`.
