# A2AHandoff

A2AHandoff is a local Windows handoff tool for coordinating **DeepSeek Harness (DSH)** and **Claude Desktop** without copying messages by hand.

It watches the bound DSH session and Claude conversation, prepares the next cross-agent handoff, writes the outgoing draft into the destination input, gives the user a cancellable delay, and submits once when the evidence is still valid.

## Current status

- Windows implementation: active.
- macOS/Linux implementation: not available yet.
- No cloud service or A2AHandoff account is required.
- Session bindings, message history, receipts and machine paths stay in the local `runtime/` directory and are intentionally excluded from Git.
- Delivery is fail-closed: ambiguous targets, changed drafts, manual intervention and uncertain submits stop automatic retry.

## Tested environment

| Component | Tested |
| --- | --- |
| Windows | Windows NT 10.0.26200 x64 |
| Windows PowerShell | 5.1 |
| Node.js | v24.21.0 |
| DeepSeek Harness | `@deepseek-ai/dsh` 0.1.5-rc.1 |
| Claude Desktop | 2.16120.0 |
| Rust toolchain | stable (rustc 1.94.0 during current validation) |

These are tested versions, not a claim that older versions are unsupported.
## Runtime requirements

For a prebuilt Windows release, the machine needs:

1. Claude Desktop, with the target Cowork conversation open when A2AHandoff needs to read or send.
2. DeepSeek Harness with its local web UI running.
3. Node.js on `PATH` with `node:zlib` Zstandard support. `scripts/check-prereqs.ps1` checks the actual API instead of assuming a version.
4. Windows PowerShell 5.1 or later.
5. Edge or Chrome for the DSH web UI by default. The process list is configurable.

Rust/Cargo are only required when building from source.

## Download (Windows)

Prebuilt packages are attached to [Releases](https://github.com/AndyTane/A2AHandoff/releases). Unzip it anywhere and keep the files together: the executables load `adapters/` from beside them, so moving `A2AHandoff.exe` on its own starts a window that can read and send nothing.

```powershell
# 1. Check this machine against the requirements above.
powershell -ExecutionPolicy Bypass -File .\scripts\check-prereqs.ps1

# 2. Create runtime\config.json and detect the local DSH data directory.
powershell -ExecutionPolicy Bypass -File .\scripts\bootstrap.ps1

# 3. Start.
.\A2AHandoff.exe --product-root .
```

Then open **Binding Configuration** in the window: select the DSH session and paste the Claude Cowork `cse_...` Session ID. Automatic handoff stays off until 自动交接 is switched on, and nothing is sent while the bindings are still the shipped placeholders.

The same three steps are in `START-HERE.txt` inside the archive. Starting the executable with no configuration at all also works — it provisions `runtime/config.json` itself and detects DSH the same way; `bootstrap.ps1` is the explicit path and prints what it found.

Packages are unsigned, so SmartScreen warns about an unknown publisher, and each release lists the SHA-256 of its download.

## Quick start from source

```powershell
git clone <repository-url>
cd A2AHandoff

# Detect a running DSH installation when possible and create local runtime config.
powershell -ExecutionPolicy Bypass -File .\scripts\bootstrap.ps1

# Optional development checks.
powershell -ExecutionPolicy Bypass -File .\scripts\check-prereqs.ps1 -Development

cargo build --release
.\target\release\A2AHandoff.exe --product-root .
```
On first launch, open **Binding Configuration**:

- Claude: enter the `cse_...` Session ID from the Claude Cowork conversation.
- DSH: select a local DSH session from the discovered list.

A2AHandoff does not need your Claude password, API key, or DSH model credentials.

## Local configuration

Local files are created under `runtime/` and are excluded from source control.

- `runtime/config.json` — machine/runtime settings.
- `runtime/bindings.json` — Claude and DSH session bindings.
- `runtime/message-templates.json` — user-editable handoff text.
- `runtime/workflow.json`, `requests/`, `receipts/`, `events.jsonl` — generated state and diagnostics; never commit them.

`scripts/bootstrap.ps1` creates `runtime/config.json` and detects `dsh_data_home`, plus `dsh_web_origin` and the DSH page title when the DSH web UI answers. The file has exactly nine fields:

| Field | Default | Purpose |
| --- | --- | --- |
| `enabled` | `false` | Automatic handoff switch; written by the app. |
| `poll_seconds` | `60` | Claude observation interval. |
| `dispatch_delay_seconds` | `10` | Delay between a verified draft and its submit. |
| `dsh_data_home` | none | DSH data directory. |
| `dsh_web_origin` | `http://127.0.0.1:3080` | Origin identifying the DSH web UI. |
| `dsh_browser_processes` | `msedge`, `chrome` | Browser processes allowed to host the DSH UI; an empty list falls back to these two. |
| `workspace` | empty | Optional expected DSH workspace; empty disables the extra cross-check. |
| `claude_host` | `claude.ai` | Host used to match the Claude Cowork conversation URL. |
| `dsh_page_title_pattern` | `DeepSeek Harness` | Title text identifying the DSH page. |

Public examples live under `examples/`. See `docs/CONFIGURATION.md` for field semantics.

## Safety model

- A handoff is prepared as a draft before the countdown starts.
- The destination session, source reply and draft are revalidated before submit.
- Manual edits or a session change cancel the pending submit.
- An uncertain submit is never retried automatically.
- Visible A2AHandoff markers are not inserted into agent conversations; correlation data stays local.
- `Restore listener` changes only the local observation baseline and does not resend a task.

## Development
```powershell
cargo test --workspace --locked
powershell -ExecutionPolicy Bypass -File .\tests\test-plain-correlation.ps1
powershell -ExecutionPolicy Bypass -File .\tests\test-reply-body.ps1
```

The integration tests use fake editor/session I/O and do not send messages to real agents.

Architecture and behavioral contracts are documented under `docs/`.

## Repository policy

The repository must not contain:

- real `cse_...` or `session-...` bindings;
- local DSH/Claude conversation content;
- anything under `runtime/` except `runtime/.gitkeep` (config, bindings, templates, receipts, requests, logs and workflow state);
- absolute developer-machine paths;
- built `.exe`/`.pdb` files or local backups.

`.gitignore` enforces these boundaries for normal Git usage. Review `docs/OPEN_SOURCE_READINESS.md` before the first public push.

## License

MIT — see [`LICENSE`](LICENSE).
