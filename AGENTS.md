# For agents

A2AHandoff is a local Windows tool that hands a task between **DeepSeek Harness (DSH)** and
**Claude Desktop** without the user copying text by hand. It watches the bound DSH session and
Claude conversation, writes the outgoing draft into the destination input, gives the user a
cancellable delay, and submits once. Delivery is fail-closed and uncertain submits are never
retried automatically.

If a user asks you to install, configure, repair or check it, this file is the whole procedure.
`skills/a2a-handoff-setup/SKILL.md` carries the same steps in the format an agent catalog reads.

## What you may do on your own

- Run the read-only checks: `scripts\check-prereqs.ps1`, `tests\test-runtime-coherence.ps1`.
- Run `scripts\bootstrap.ps1`. It writes `runtime\config.json` — machine-local, no secrets, and
  it refuses to overwrite an existing file unless given `-Force`.
- Start the app: `A2AHandoff.exe --product-root .`
- Read `runtime\state.json`, `runtime\events.jsonl` and `runtime\receipts\`.

## What needs the user's decision first

- **Binding a session.** The Claude `cse_...` Session ID and the DSH session are the user's
  identity for this tool, not a machine setting. Show them what you found and let them choose;
  do not pick a conversation for them. This is the one step that needs the window anyway —
  **Binding Configuration** (绑定配置) is where they enter it.

## What you must never do

- Click 发送给 Claude / 发送给 DSH, press 取消本次, or press 重试本次投递 on the user's behalf.
- Write into `runtime\commands\` to make the runtime send something: pressing send is the user's
  decision, and this tool exists so that a handoff is deliberate.
- Rewrite `runtime\receipts\`, `runtime\workflow.json` or `runtime\events.jsonl` to "fix" a
  delivery. They are the record of what actually left the machine.

## Install, from a downloaded package

```powershell
# 1. Requirements: Claude Desktop, DSH with its web UI running, Node.js with node:zlib zstd
#    support, Windows PowerShell 5.1, Edge or Chrome.
powershell -ExecutionPolicy Bypass -File .\scripts\check-prereqs.ps1

# 2. Create runtime\config.json and detect the DSH data directory.
powershell -ExecutionPolicy Bypass -File .\scripts\bootstrap.ps1

# 3. Start.
.\A2AHandoff.exe --product-root .
```

Keep the package files together: both executables load `adapters\` from beside them.

Starting `A2AHandoff.exe` with no arguments also works — it finds its own folder, writes
`runtime\config.json` and detects DSH itself. `bootstrap.ps1` is the explicit path and prints
what it detected.

From source, the same steps after `cargo build --release --locked`.

## What a correct first run looks like

- Both cards read **未绑定** and the banner reads **还不能开始：会话尚未绑定** with a button that
  opens 绑定配置. That is the shipped state, not a fault.
- `runtime\bindings.json` holds placeholders (`cse_unconfigured`, `session-unconfigured`). A
  placeholder is not a binding; the window says so.
- After the user binds both sides: the Claude card reads **已绑定** with a green
  「会话已核验」, and its 当前任务 is the conversation name the runtime is actually reading.
- 监听 must be on for observation, and 自动交接 must be switched on for automatic handoff.
  Neither is on in a fresh install, and nothing is sent while the bindings are placeholders.

## When something looks wrong

Evidence order — later items are more trustworthy than earlier ones:

1. `runtime\state.json` — `phase`, `status_text`, `detail`, `claude_ok`, `dsh_ok`.
2. `runtime\workflow.json` — `pending`, `last_delivery`, the watermarks.
3. `runtime\events.jsonl` — one JSON object per line. `send_refused`, `delivery_receipt`,
   `draft_ready`, `manual_intervention` explain a stuck handoff. Read this before guessing.
4. `runtime\receipts\<id>.json` — the per-delivery outcome. `sent` and `submit_uncertain` are
   terminal; everything else means the draft did not leave.
5. `runtime\runtime-stderr.log` — one line per crash. Read it as UTF-8 or it looks like mojibake.

**`state.json` is what the runtime publishes, not what the window shows.** If the user says the
window says something, capture the window (see `artifacts\ui-review\capture-window.ps1` in a
developer tree) — do not conclude anything about the UI from `state.json`.

Run `tests\test-runtime-coherence.ps1` first: exit 0 means every invariant held (or nothing is
running), exit 1 prints the violations. It never writes or sends.

## Working on the source

```powershell
cargo fmt --all
cargo test --workspace --locked
cargo build --release --locked --target-dir target/draft-first   # the running app locks target\release
Copy-Item target\draft-first\release\*.exe dist\ -Force
powershell -ExecutionPolicy Bypass -File scripts\install-build.ps1   # exits 2 while the app runs
powershell -ExecutionPolicy Bypass -File scripts\package-release.ps1 # builds the release zip
```

Traps that have actually bitten:

- A `.ps1` containing non-ASCII without a **UTF-8 BOM** does not parse on PowerShell 5.1.
  `tests\test-script-encoding.ps1` guards every script.
- The window's child controls are keyed by **id**, so a banner button must carry a banner id
  (`ID_BANNER_*`). Reusing a top-bar id silently moves that control into the banner.
- A test that shells out inherits the child's exit code; state the script's own result or a
  passing run reports itself as failed.
- Do not hold `runtime\state.json` open: the runtime publishes by temp-file-and-rename, and a
  reader that keeps the file open makes the rename fail.

## Where the rest is

`README.md` for users · `docs/` for behaviour and configuration · `tests/README.md` for what the
suite covers · `docs/CONFIGURATION.md` for every config field.
