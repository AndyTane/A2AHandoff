# Contributing

## Development checks

Before submitting a change:

```powershell
cargo fmt --all -- --check
cargo test --workspace --locked
powershell -ExecutionPolicy Bypass -File .\tests\test-plain-correlation.ps1
powershell -ExecutionPolicy Bypass -File .\tests\test-reply-body.ps1
```

Changes to delivery, UI Automation, idempotency or manual-intervention behavior should include a regression test.

Never use a real agent conversation for an automated CI test. Integration tests should inject fixture observations and fake editor I/O.

Do not commit anything under `runtime/`, `backups/`, `artifacts/` or `target/`.
