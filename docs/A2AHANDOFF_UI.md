# A2AHandoff UI Product Spec

## Product identity

A2AHandoff is a compact local control surface for Claude Desktop ↔ DeepSeek Harness handoff on Windows.

Principles: evidence-first, low-friction, explicit state, no hidden retries, no decorative dashboard complexity.

## Main window

- Native Windows UI; no Electron/WebView runtime.
- Dark theme.
- Two bound-agent cards: Claude Desktop and DeepSeek Harness.
- DSH card shows the native turn state and a meaningful listener state, not a continuously resetting heartbeat counter.
- Main actions: Send to DSH, Send to Claude, Cancel current handoff.
- Header actions: automatic mode, Restore listener, Binding configuration, Diagnostics.

## Delivery state

The destination draft is written first. Only after exact read-back succeeds does the cancellable dispatch countdown begin.

During the countdown:

- the destination input already contains the outgoing text;
- Cancel stops the submit and preserves the draft;
- editing the draft or changing the bound conversation cancels the submit;
- expiry revalidates evidence and performs one native submit action.

An uncertain submit stops. It does not create a second-confirmation loop and is never retried blindly.
## Listener and automatic mode

- Automatic mode schedules observation using the configured poll interval.
- Footer shows the actual next poll deadline/countdown.
- Restore listener re-baselines the current bound DSH session without resending a task or clearing receipts.
- If DSH is running, restore attaches to the current turn; if idle, it waits for the next turn.

## Configuration surfaces

- Binding configuration: Claude `cse_...` Session ID + DSH session chooser.
- Handoff text: independent Claude → DSH and DSH → Claude message configuration.
- Poll interval: editable from the main window.
- Environment/machine settings remain in local runtime configuration.

## Platform status

Windows is the only implemented shell today. Core state-machine code is kept platform-neutral where practical, but macOS/Linux UI and adapters are future work rather than current supported targets.
