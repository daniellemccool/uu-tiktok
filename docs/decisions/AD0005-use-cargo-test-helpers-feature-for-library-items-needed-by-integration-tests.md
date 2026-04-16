---
adr_id: "0005"
comments:
    - author: Danielle McCool
      comment: "1"
      date: "2026-04-16 23:49:54"
links:
    precedes: []
    succeeds: []
status: decided
tags: []
title: Use Cargo test-helpers feature for library items needed by integration tests
---

## <a name="question"></a> Context and Problem Statement

T9 introduced the first integration test (`tests/state_ingest.rs`) that needs to call test-only library items: a `get_video_for_test(...)` accessor returning a `VideoRow` snapshot for assertion. These helpers should not appear in the public library API for downstream consumers, but integration tests live in a separate compilation unit (`tests/`) so `#[cfg(test)]` alone does not reach them. We need a convention for exposing test-only library items to integration tests, before T10/T11/T12 repeat the pattern in their own ways.

## <a name="options"></a> Considered Options

1. <a name="option-1"></a> Cargo feature flag `test-helpers` with `#[cfg(any(test, feature = "test-helpers"))]` gating; per-test `required-features = ["test-helpers"]`
2. <a name="option-2"></a> Separate workspace sub-crate (`uu-tiktok-test-helpers`) that re-exports test fixtures
3. <a name="option-3"></a> `pub(crate)` items combined with a `#[cfg(test)]` re-export module
4. <a name="option-4"></a> Inline raw rusqlite queries in each integration test instead of helpers
5. <a name="option-5"></a> Drop integration tests entirely; rely only on `#[cfg(test)]` unit tests inside the lib

## <a name="criteria"></a> Decision Drivers

Test-only items must not appear in the public library API for downstream consumers. Integration tests in `tests/` must be able to reach them. Pattern must be cheap to add per-helper and discoverable in the source. Must compose with AD0002's dead-code suppression for the bin+lib asymmetry. No new dependencies.



## <a name="outcome"></a> Decision Outcome
We decided for [Option 1](#option-1) because: Option 1 is the conventional Rust idiom for "library item visible to integration tests but not to public consumers." The library item gets #[cfg(any(test, feature = "test-helpers"))] gating; the integration test gets required-features = ["test-helpers"] in its [[test]] block. Cheap to add per-helper, discoverable in the source, no new dependencies. Rejected option 2 (sub-crate): overkill for one helper today and adds workspace structure cost; revisit only if test fixtures grow into a shared library across multiple downstream test suites. Rejected option 3 (pub(crate) + cfg(test) re-export): cfg(test) does not extend to integration tests in tests/ because each integration test is a separate compilation unit, so the helpers would still be unreachable. Rejected option 4 (inline raw queries): leaks rusqlite into every integration test, helpers drift from impl, no shared assertion shape. Rejected option 5 (drop integration tests): integration tests give end-to-end coverage that #[cfg(test)] unit tests cannot - real SQLite file on disk, full Store::open path including PRAGMA application, FK enforcement, the actual lib API surface as downstream consumers see it. Trigger: T9 introduced this pattern for get_video_for_test and VideoRow. T10 will add row inspectors after claim_next, T11 will add fake fetcher trait impls, T12 will add transcribe-mock helpers. Recording the convention now to prevent ad-hoc divergence. Consequences: positive - cheap, idiomatic, no new deps, reuses standard cargo machinery. Negative - enabling --features test-helpers at workspace level (e.g. cargo clippy --all-targets --features test-helpers) leaks the feature into the bin compilation since cargo features unify across the build graph, so cfg-gated items get pulled into the bin compilation but never called there, firing dead_code. Resolved by AD0002 #[allow(dead_code)] with a justification comment that explicitly names the bin-firing-with-feature dynamic. Negative - each new integration test file must add its own [[test]] required-features = ["test-helpers"] block. Convention: every new integration test file opts in by default at the cost of one extra Cargo.toml block per file; do not gate per-feature subsets within a single integration test file. Trade-off: chose feature flag over sub-crate because Plan A scale does not justify the workspace restructuring cost, with the option to switch to a sub-crate later if test fixtures multiply.

## <a name="comments"></a> Comments
<a name="comment-1"></a>1. (2026-04-16 23:49:54) Danielle McCool: marked decision as decided
