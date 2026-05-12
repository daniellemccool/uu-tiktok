---
adr_id: "0011"
comments:
    - author: Danielle McCool
      comment: "1"
      date: "2026-05-12 13:06:05"
links:
    precedes: []
    succeeds: []
status: decided
tags: []
title: Spin-down operational practice for dev workspace
---

## <a name="question"></a> Context and Problem Statement

Plan A's prior SRC deployment burned ~133 CPU-core-hours over 2.5 idle days. The dev grant's 15K CPU-core-h budget cannot accommodate continuous workspace running over 12 months. What is the canonical operational practice for stopping the workspace between batches?

## <a name="options"></a> Considered Options

1. <a name="option-1"></a> Pause via SRC portal between every working session
2. <a name="option-2"></a> Delete between every session; re-provision when needed
3. <a name="option-3"></a> Always-on workspace; accept the burn rate
4. <a name="option-4"></a> Auto-pause via SRC's scheduled actions (if available)

## <a name="criteria"></a> Decision Drivers

Budget math (15K CPU-core-h / 12 months). Grant-wallet pause behavior (workspace-lifecycle.md:17-20: pause on grant wallets charges zero CPU/GPU and zero storage). Recovery friction. Operational checklist must be implementable now.

## <a name="outcome"></a> Decision Outcome
We decided for [Option 1](#option-1) because: On grant-based wallets (Workstream 1's), pause charges zero CPU/GPU AND zero storage (workspace-lifecycle.md:17-20). Resume reattaches storage and restarts the workspace. Operational checklist before pause: stop active batches (Ctrl+C process); verify no users logged in; confirm no in_progress rows (Epic 4's status subcommand will validate this — see AD0017). After resume: verify SSH; verify nvidia-smi shows the A10; verify storage volume mounted; rebuild any state lost from local disk. Rejected alternatives: Option 2 (delete + re-provision) forces ~10–15 min re-provisioning + reinstall each session, high friction. Option 3 (always-on) incompatible with budget math — 15K CPU-core-h / 12 months cannot sustain continuous running. Option 4 (auto-pause via SRC scheduled actions) — investigate later; manual pause is sufficient for Epic 1, and SRC scheduled-action support hasn't been verified for this workspace shape. Cross-reference AD0017: the status subcommand from Epic 4 implements the safe-to-pause check.

## <a name="comments"></a> Comments
<a name="comment-1"></a>1. (2026-05-12 13:06:05) Danielle McCool: marked decision as decided
