---
adr_id: "0017"
comments:
    - author: Danielle McCool
      comment: "1"
      date: "2026-05-12 13:07:38"
links:
    precedes: []
    succeeds: []
status: decided
tags: []
title: Operational done contract for batch validation
---

## <a name="question"></a> Context and Problem Statement

When can an operator declare a batch 'done' and safe to spin down the workspace? Plan A's exit-3 mechanism (process returned 3 = nothing to claim) is insufficient — it doesn't verify artifacts on disk or schema compliance.

## <a name="options"></a> Considered Options

1. <a name="option-1"></a> Define the contract in this ADR; implement in Epic 4's status subcommand. Contract = counts by status (all terminal), all succeeded rows have artifacts on disk, all raw_signals.schema_version match expected.
2. <a name="option-2"></a> Implement in Epic 1. Adds scope to Epic 1.
3. <a name="option-3"></a> Don't define until Epic 4. Risk: implementer of Epic 4 has no contract to fulfill.

## <a name="criteria"></a> Decision Drivers

Contract must be precise enough to implement. Must be Epic-1-draftable but Epic-4-implementable. Must integrate with AD0011's spin-down practice.

## <a name="outcome"></a> Decision Outcome
We decided for [Option 1](#option-1) because: ADR drafted now (Epic 1) so the status subcommand has a clear contract to implement. Subcommand itself lands in Epic 4. Contract (formal): Counts by status — every row in videos has terminal status (no in_progress, no pending unless explicitly skipped via --max-videos). Artifact existence — every succeeded row has .txt and .json at the sharded path. Schema-version check — every .json's raw_signals.schema_version matches EXPECTED_RAW_SIGNALS_SCHEMA_VERSION. Optional — artifact backup to Research Drive completed (if configured). Pause-safe — all of the above pass AND no in_progress rows pending recovery. Cross-references: AD0011 (spin-down practice) consumes the pause-safe check; AD0010 defines raw_signals.schema_version that the schema-version check validates. Rejected alternatives: Option 2 (implement in Epic 1) — adds scope to Epic 1 without delivering the user-facing value that lives in Epic 4 (operator-facing status command); the contract is small enough to draft now but the implementation reads DB and filesystem state that Epic 4's subcommand harness owns. Option 3 (don't define until Epic 4) — leaves Epic 4's implementer with no contract to fulfill and risks the contract being shaped to fit whatever Epic 4's implementation happens to do, rather than what operators actually need.

## <a name="comments"></a> Comments
<a name="comment-1"></a>1. (2026-05-12 13:07:38) Danielle McCool: marked decision as decided
