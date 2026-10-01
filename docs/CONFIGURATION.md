# Configuration

A2AHandoff separates machine settings, session bindings, and message text so that local identity data never needs to be committed.

## `runtime/config.json`

The runtime config has exactly nine fields. Deprecated keys are ignored on load and are never written back.

### Basic settings

| Key | Required | Default | Purpose |
| --- | --- | --- | --- |
| `dsh_data_home` | yes | none | DSH data directory containing `storages/session_projcache/sessions` and native session logs. |
| `dsh_web_origin` | no | `http://127.0.0.1:3080` | Origin used to identify the DSH web UI. Persisted as `scheme://host:port` with the scheme default port resolved (`http` → `80`, `https` → `443`). |
| `poll_seconds` | no | `60` | Claude observation interval, 1–7200 seconds. |
| `dispatch_delay_seconds` | no | `10` | Delay after a verified draft is written and before it is submitted, 1–120 seconds. |

### Advanced settings

| Key | Required | Default | Purpose |
| --- | --- | --- | --- |
| `dsh_browser_processes` | no | `["msedge", "chrome"]` | Browser process names allowed to host the DSH UI; names are trimmed and lower-cased. An empty or unusable list falls back to `msedge`, `chrome`. |
| `workspace` | no | `""` | Optional defense-in-depth check against the DSH session `cwd`. Empty disables this extra check. |
| `claude_host` | no | `claude.ai` | Host whose Cowork conversation URL is bound to the Claude desktop window; stored lower-cased. An empty value falls back to `claude.ai`. |
| `dsh_page_title_pattern` | no | `DeepSeek Harness` | Page title text that identifies the DSH web UI. An empty value falls back to `DeepSeek Harness`. |

`enabled` (default `false`) is the automatic handoff switch. It is written by the app rather than by `scripts/bootstrap.ps1`.

`scripts/bootstrap.ps1` creates this file, attempts to detect `dsh_data_home` from a running DSH process and, when the DSH web UI answers, detects `dsh_web_origin` and the page title as well. Detection is optional: on any failure the defaults above are used. An existing file is never overwritten unless `-Force` is passed.

## `runtime/bindings.json`

This file is user-specific and must not be committed.

- `claude_session`: Claude Cowork Session ID (`cse_...`). This is the stable identity used for Claude URL matching.
- `dsh_session`: DSH Session ID (`session-...`).
- `claude_title` / `claude_window`: retained presentation metadata for compatibility; users do not need to maintain them manually.
## `runtime/message-templates.json`

This file controls visible text sent between agents.

- Claude → DSH: only the configured instruction is sent. Claude's full reply is used locally for dedup/correlation and is not pasted into DSH.
- DSH → Claude: the DSH result body is inserted between the configured prefix and suffix.
- No visible A2AHandoff delivery ID is added to conversations.

## What is intentionally not configurable

Some values are adapter contracts rather than user preferences:

- Claude desktop process name: `claude`.
- DSH native storage layout under `dsh_data_home`.
- Cowork URL matching built on the configured `claude_host` (`/cowork/<claude_session>`).
- DSH page identity matching: the bound session title must start with the observed DSH title and contain `dsh_page_title_pattern`.

If those upstream products change, the appropriate adapter should be updated instead of exposing every selector as a user-facing setting.

## Removed/deprecated settings

These keys still load without error from an existing file: they are ignored and are never written again.

- `task_file`: no longer a runtime setting. Put the task filename/path in the Claude → DSH message template.
- `mode`: runtime mode is internal state rather than user configuration; it lives in `runtime/state.json`.
- `next_poll_at_ms`: scheduler state, not configuration; it lives in `runtime/state.json`.

The hard-coded product root was removed earlier: the UI passes `--product-root` and the runtime falls back only to the current working directory.
