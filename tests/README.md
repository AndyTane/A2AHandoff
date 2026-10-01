# Public tests

The repository intentionally keeps only deterministic tests that do not send messages to real Claude or DSH sessions.

- `test-plain-correlation.ps1` — full-content Claude user-message correlation.
- `test-reply-body.ps1` — Claude UI Automation response-body parsing using in-memory fixtures.
- `test-claude-anchor-stability.ps1` — the manual-intervention anchor hash must not move when Claude re-renders a message's view chrome (collapse indicator, expand control, action bar), while a real content edit must still move it. Guards the fix for auto handoff pausing itself after a successful delivery.
- `test-adapter-config.ps1` — adapters read every machine-specific value from `runtime/config.json` with the documented fallbacks.
- `test-bootstrap-firstrun.ps1` — first-run detection, config seeding and explicit-argument handling.
- `test-script-encoding.ps1` — every PowerShell source parses as Windows PowerShell 5.1 would read it, and every non-ASCII file carries a UTF-8 BOM.
- `test-runtime-coherence.ps1` — asserts invariants against a **running** instance: state freshness, that the status text never claims a side is executing while it is idle, that watermarks do not sit ahead of reality, that every receipt state is known, and that no handoff is stuck in preparation. Read-only; `-SkipObservers` runs the state half only (what CI does), and with no instance present it skips with exit 0 unless `-RequireRunning` is given.
- `staged-flow-integration.py` — real runtime state machine with fake session/editor I/O.
- `test-preparation-hold-integration.py` — occupied-draft hold/retry behavior with fake I/O.

Many one-off debugging and migration scripts may exist in a developer working tree. `.gitignore` excludes them from the public repository; each deterministic test above is whitelisted explicitly.
