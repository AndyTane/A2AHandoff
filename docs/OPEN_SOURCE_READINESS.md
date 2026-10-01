# Open-source readiness

Status: **the source tree is public and carries the chosen license (MIT). Release policy is still open.**

## Completed in the repository

- Runtime state and machine-specific files are excluded by `.gitignore`.
- Build outputs, local backups, UI review artifacts and one-off migration/debug scripts are excluded.
- Public example files exist under `examples/`.
- Windows runtime and build prerequisites are documented.
- A bootstrap script creates machine-local configuration without committing it.
- DSH Web origin, browser process names, Claude host, and the DSH page title pattern are configurable.
- DSH workspace verification is optional.
- The runtime no longer falls back to a developer-specific absolute product path.
- The retired V0/shadow Windows adapter is removed from the Cargo workspace and excluded from Git.
- README now reflects the actual Windows-only implementation.
- MIT license in place (`LICENSE`), and the README license section states it.

## Blocking before a public repository

1. ~~Choose a license.~~ Chosen: **MIT** — `LICENSE`.
2. Decide the final GitHub repository name and description.
3. Decide whether releases will be source-only initially or include unsigned Windows binaries.

## Recommended before the first tagged release

- Add a first-run environment screen so users do not need `scripts/bootstrap.ps1` for `dsh_data_home`.
- Test on a second clean Windows machine/user profile.
- Test at least one DSH version other than the currently validated 0.1.5-rc.1.
- Test after a Claude Desktop update because UI Automation structure is an upstream compatibility surface.
- Consider Windows code signing if distributing binaries broadly.
## Configuration boundary

Keep these user-configurable:

- DSH data directory.
- Optional expected workspace.
- DSH Web origin.
- Allowed DSH browser process names.
- Claude host used to match the Cowork conversation URL.
- DSH page title pattern used to identify the DSH web UI.
- polling interval.
- draft-to-submit delay.
- Claude and DSH session bindings.
- handoff message templates.

Keep these as adapter implementation details unless upstream products force a change:

- Claude UI Automation selectors and the Cowork URL structure derived from `claude_host`.
- DSH session-log schema and native storage subpaths.
- send-button/UI selectors.
- correlation and idempotency rules.

## Privacy review

A public repository must never include the contents of the local `runtime/` directory. It can contain conversation-derived text, session IDs, hashes, local project paths and delivery diagnostics.

Before every release/push, run a staged-file scan for:

- `cse_` and `session-` values that are not fixtures/examples;
- Windows absolute paths;
- `token`, `password`, `secret`, `api_key`, `Bearer`;
- generated `.jsonl`, receipts and requests.
