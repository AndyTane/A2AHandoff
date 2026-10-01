# Security

A2AHandoff automates local desktop inputs. Bugs in target selection or correlation can therefore send text to the wrong local conversation.

## Security boundaries

- Session IDs and conversation-derived data are local runtime data and must not be committed.
- A2AHandoff does not require Claude or DSH credentials.
- Delivery fails closed when the destination, draft, source reply or ownership evidence changes.
- Uncertain submits are not retried automatically.
- The public test suite must use fixtures/fake editor I/O and must not send to real agents.

## Reporting

Do not include real session IDs, conversation text, credentials or absolute personal paths in a public issue. Redact local identifiers and attach the smallest diagnostic excerpt needed to reproduce the problem.
