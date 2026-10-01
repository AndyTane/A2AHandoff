# Manual Intervention Contract

Automation must yield control when a human sends a message directly in an agent conversation.

## Claude correlation

When the tool sends a DSH result to Claude it records a HandoffAnchor:
- target Claude session
- exact tool-sent user-message fingerprint
- target user-message index
- dispatch identity

A Claude assistant response is eligible for automatic Claude -> DSH delivery only when:
1. ownership is Tool;
2. the latest Claude user message fingerprint equals the anchor fingerprint;
3. the latest Claude user message index equals the anchored index;
4. the assistant response is after that anchored user message;
5. the Claude reply itself has not already been delivered to DSH.

If a different user message appears after the anchor, automation enters PAUSED_BY_USER.
If ownership/correlation cannot be proven, automation enters HOLD.

## Resume

Automation never guesses a resume point. The user must explicitly resume from a chosen current reply. Resume creates a new tool-owned anchor; subsequent replies can again participate in automation.

The same ownership model will apply symmetrically to direct manual intervention in DSH.
