# Handoff Core V1 Behavioral Contract

## Idempotency
DSH -> Claude: reply content hash is the content dedup key; session/external-turn metadata is evidence.
Claude -> DSH: Claude response content hash is the content dedup key. DSH_TASKS.MD hash is diagnostic only.

## DSH turns
DSH may self-progress any number of internal turns between external handoffs. Core MUST NOT require physical turn adjacency.

## Scheduler
Each poll cycle performs at most one cross-agent delivery:
1. Validated unconsumed DSH result -> Claude.
2. Otherwise, if DSH is idle, validated unconsumed Claude response -> DSH.
3. Otherwise no delivery.
Contradictory or insufficient evidence produces HOLD, never automatic retry.

## Required parity scenarios
1. Claude A -> DSH N; DSH self-progresses N+1..N+K; Claude B -> DSH N+K+1.
2. Repeated Claude A is blocked regardless of DSH current turn.
3. Repeated DSH reply A is blocked regardless of intervening turns.
4. DSH busy prevents new external dispatch.
5. Uncertain send receipt never causes automatic retry.
6. Closing the application stops scheduler/listeners it owns.
7. Logs, snapshots, and in-memory history remain bounded.
