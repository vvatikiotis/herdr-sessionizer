# Full specification-to-test coverage plan

> Historical planning artifact: the checklist below records the intended audit sequence and is not a current task list or execution report. Completion evidence and remaining limitations are recorded in `docs/spec-test-coverage-map.md`; do not infer completion or current status from the plan alone.

## Goal and limits

Map every normative requirement in SPEC.md to observable assertions. Add missing coverage at the highest existing boundary. This is a plan, not authorization to implement. Do not add new requirements, speculative abstractions, or style-only refactoring.

Full coverage means every in-scope requirement has a suitable assertion and a recorded passing run, or an explicit environment limitation. It does not mean every possible input/platform is tested. Never count a limitation as a pass. Caller-owned popup configuration and out-of-scope behavior are not new plugin test requirements.

## 1. Establish a traceability baseline

- Assign stable requirement IDs to normative statements without changing their meaning.
- Create a coverage map with requirement ID, spec section, assertion/test reference, test boundary, evidence, platform, and remaining gap.
- Separate required behavior from historical notes and incidental implementation details. Excluded guarantees such as concurrency serialization do not require new product behavior.
- Read assertions, not just test names. Distinguish candidate-stream assertions from rendered UI, highlighted targets, and accepted actions.
- Inventory existing coverage before adding tests. Retain already adequate tests rather than duplicating them.

Deliverable: complete requirement map with covered, partial, missing, or environment-limited status.

## 2. Close executable-boundary gaps

Use the compiled binary, temporary deployment/home/state, and controlled Herdr/fzf stubs already used by the suite.

- Shell-pool failure: unsuccessful delegate produces failure and never retries ordinary workspace creation or emits a successful directory result.
- Workspace ordering: one scenario includes previous, current, older visited, and unvisited workspaces, with nested agents and pool exclusion.
- Directory identity: assert a usable foreground_cwd when cwd is absent/null and the first-pane fallback rule. Assert selection-time access failure for an unavailable explicit include and no creation/focus on that failure.
- Snapshot stability: change the stub's later agent metadata and verify retries still use captured IDs rather than retargeting or refreshing.
- Defaults: deep fixture distinguishes built-in depth three from the checked-in editable depth four, without treating the editable value as the default contract. (Completed in the recorded audit; see the coverage map.)
- Pane-exit: verify the specified pool-only/no-surviving-user no-creation case, still-existing source workspace, existing user focus preservation, and target selection. Add event-validation/timeout/focus-failure checks only where the current spec explicitly requires that behavior; do not invent an error contract from source branches.
- Error-view launch failure: strengthen explicit owned-pane closure, no-navigation/creation, and temporary-resource assertions where missing.
- Add remaining schema/path/symlink cases only when the requirement map finds no suitable assertion.

Deliverable: missing behavioral assertions, demonstrated red before necessary product fixes, then green.

## 3. Assert manifest contracts

- Validate plugin identifier, minimum version, supported platforms, startup tracking, workspace-focus tracking, pane-exit hook, and absence of a plugin-owned picker binding.
- Use the checked-in manifest as input, not a duplicate fixture or a search against comments.
- Keep static manifest validation distinct from actual event-delivery evidence supplied by the isolated Herdr regression.

Deliverable: direct assertions for the manifest requirements already in the spec.

## 4. Close real-fzf interaction gaps

Extend the existing isolated PTY tests, preserving separate stdout/stderr where required and a correctly established controlling terminal.

- Omitted-workspace warning is rendered but cannot be selected, searched as a candidate, or produce navigation.
- Retained workspace rows remain present for empty, one-letter/multiline-match, agent, directory-only, and zero-directory-match queries. Check actual displayed/candidate identity rather than only the returned directory.
- Assert first-match display-order priority, agent filtering, filtered-row positions, and directory fallback without accepting before asynchronous filtering settles.
- Exercise Ctrl-Space acceptance and cancellation with no navigation/creation; distinguish successful empty selection from nonzero cancellation.
- Check required layout/options directly where invocation assertions suffice; reserve real-terminal checks for behavior that cannot be established from arguments alone.
- Keep the six passing configuration/discovery dismissal cases. Assert a live view before each key, normal exit afterward, failure status, original stderr, empty stdout and cleanup. Continue draining terminal output while awaiting exit. Timeouts/forced termination fail, never silently skip.
- Verify stale-agent retry still uses captured state and does not accidentally accept the following workspace.

Deliverable: bounded, repeatable real-fzf regressions with rendered/accepted identity evidence, not just matched transcript text or constructed shell bindings.

## 5. Filesystem/platform limitations

- Attempt unreadable-root and permission-denied target fixtures under a non-privileged test identity where supported. (The recorded macOS audit tested unreadable roots; permission-denied symlink-target behavior was unsupported on that host.)
- Confirm the fixture truly denies access before accepting the test result. If it does not, report a named limitation with reason.
- Keep deterministic symlink-loop/non-NotFound coverage separate from permission-specific evidence.
- Record supported host/Herdr/fzf versions and avoid implying Linux coverage from macOS runs. Do not introduce privileged environment changes merely to obtain green tests.

Deliverable: passing permission scenarios where reproducible, explicit limitations otherwise.

## 6. Final verification and review

- Run targeted tests throughout, then all Rust tests in debug/release, formatting, Clippy, and all isolated live scripts.
- Review the requirement map independently against actual assertions. No requirement may silently disappear between the spec and test plan.
- Record limitations, failures, and test commands. Update SPEC.md evidence and test counts only from completed runs.
- Review code changes for spec conformance and the small-safe-change/YAGNI rules. Commit only approved changes; preserve unrelated working-tree edits.

Completion: every normative requirement is mapped; every uncovered behavior has an approved test or explicit limitation; all required executable/live checks pass. No live session modification, deployment, or external publishing is part of this plan.

## Execution order

Baseline map → executable gaps and manifest assertions → real-fzf gaps → final complete verification. Filesystem capability checks can run alongside executable tests. Avoid parallel edits to the shared live-picker harness; use one owner for that file. Stop for clarification if a requirement has no safe, unambiguous test interpretation.
