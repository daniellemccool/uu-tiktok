---
adr_id: "0002"
comments:
    - author: Danielle McCool
      comment: "1"
      date: "2026-04-16 21:59:07"
links:
    precedes: []
    succeeds: []
status: decided
tags: []
title: Dead-code suppression strategy and deferred binary-library restructuring
---

## <a name="question"></a> Context and Problem Statement

Plan A's pattern of declaring 'mod foo' in both src/lib.rs (as 'pub mod foo') and src/main.rs causes the compiler's dead_code lint to fire on items in modules that exist as scaffolding for future tasks but aren't yet consumed by main.rs. Under our 'cargo clippy --all-targets -- -D warnings' policy this becomes a build failure. How should we suppress these dead-code warnings in a way that (a) ensures stale suppressions are removed when the consuming code lands, and (b) does not require restructuring the bin/lib pattern mid-Plan-A?

## <a name="options"></a> Considered Options

1. <a name="option-1"></a> #[allow(dead_code)] with mandatory justification comment; manual cleanup via per-task-file checkpoint
2. <a name="option-2"></a> #[expect(dead_code)] (Rust 1.81+) as a self-cleaning forcing function
3. <a name="option-3"></a> Drop the duplicate 'mod foo' from main.rs for not-yet-consumed modules; access via uu_tiktok::foo when main needs it
4. <a name="option-4"></a> Adopt a thin-binary fat-library pattern; main.rs imports everything via 'use uu_tiktok::...'; all pub mod declarations live in lib.rs only

## <a name="criteria"></a> Decision Drivers

The build must stay green at every commit. Stale #[allow(dead_code)] annotations risk hiding genuine dead-code regressions if not removed promptly. Plan A is mid-execution; restructuring mid-flight requires per-task-file edits and risks introducing bugs in already-shipped tasks. The plan explicitly anticipates structural reassessment between Plan A and Plan B. Subagent dispatch and per-task-file structure are stable and should not be churned unnecessarily.

## <a name="outcome"></a> Decision Outcome
We decided for [Option 1](#option-1) because: Option 2 was empirically tested and rejected: in our bin+lib structure, pub items in the library are exempt from the dead_code lint, so #[expect(dead_code)] is unfulfilled in the library compilation and emits 'unfulfilled_lint_expectations', which is fatal under -D warnings. Confirmed against src/canonical.rs: 'error: this lint expectation is unfulfilled'. Option 3 was tested and works (clippy clean, all tests pass with mod canonical removed from main.rs), but creates an inconsistent pattern within main.rs (some modules via 'mod', others via 'use uu_tiktok::') and does not eliminate the duplicate-types footgun (where crate::cli::Cli and uu_tiktok::cli::Cli are technically distinct types compiled from the same source). Option 4 is the most structurally sound and would eliminate dead-code suppression entirely, but is the most invasive change to make mid-Plan-A and would force per-task-file edits across T6, T7, T8, T11, T12, and possibly T15. The plan explicitly anticipates structural reassessment between Plan A and Plan B; that is the appropriate moment to decide between options 3 and 4. For the remainder of Plan A, option 1 is sufficient. Cleanup discipline: each per-task file for a task that consumes a previously-dead type must include a 'remove the now-stale #[allow(dead_code)] on X' step. Optional backstop: periodic 'rg allow.dead_code src/' audit.

## <a name="comments"></a> Comments
<a name="comment-1"></a>1. (2026-04-16 21:59:07) Danielle McCool: marked decision as decided
