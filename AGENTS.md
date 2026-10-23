## Agent skills

### Issue tracker

Issues and specs live in GitHub Issues for vvatikiotis/herdr-sessionizer. See `docs/agents/issue-tracker.md`.

### Triage labels

Use the five default triage labels. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context layout: root glossary and docs/adr/. See `docs/agents/domain.md`.

## Coding standards

- Prefer small safe changes over style-only churn.
- Apply YAGNI ("You Aren't Gonna Need It"): implement only what the approved requirements demand. Do not add speculative features, abstractions, configuration, or extensibility.
- When requirements are ambiguous, choose the smallest conforming change. If no safe interpretation exists, stop and request clarification rather than expanding the scope.
