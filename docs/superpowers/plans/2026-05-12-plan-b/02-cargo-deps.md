# Task 2 — Add whisper-rs (cuda feature-gated) and hound to Cargo.toml

**Goal:** Add the two new dependencies Plan B Epic 1 needs, behind a `cuda` cargo feature so local CPU builds still work. Pin both `whisper-rs` crate version and document the whisper.cpp commit it tracks (per AD0009).

**ADRs touched:** AD0009 (whisper-rs version-pin), AD0014 (audio-input invariant → `hound`).

**Files:**
- Modify: `Cargo.toml`
- Modify: `docs/decisions/AD0009-*.md` (post-decide, record the exact pinned versions)

---

- [ ] **Step 1: Look up the current latest stable `whisper-rs` and the whisper.cpp commit it ships**

Run:
```bash
cargo search whisper-rs --limit 5
```

Expected: a list of versions; pick the latest non-pre-release. As of writing this plan, expect something like `0.13.x` or later. **Record the exact version chosen.**

Next, look up which whisper.cpp commit the chosen `whisper-rs` version tracks. The crate's `build.rs` or `whisper-rs-sys/Cargo.toml` should reveal this — typically a git submodule reference. Run:
```bash
cargo info whisper-rs
```
Or check the crate's source on docs.rs. **Record the whisper.cpp commit SHA or tag.**

- [ ] **Step 2: Write the Cargo.toml change**

Modify `Cargo.toml`:

```toml
[dependencies]
# ... existing dependencies ...
whisper-rs = { version = "=0.13.X", default-features = false }
hound = "3.5"

[features]
default = []
cuda = ["whisper-rs/cuda"]
test-helpers = []
```

(Replace `=0.13.X` with the exact version recorded in Step 1.)

The `=` prefix pins exact version (no semver-compatible upgrades). The `cuda` feature opts into the GPU build; on CPU it falls back to whisper.cpp's CPU backend (slow but functional). The `test-helpers` feature is unchanged from Plan A.

- [ ] **Step 3: Verify it builds on the local machine (no `cuda` feature)**

Run:
```bash
cargo build
```

Expected: a clean build. First-time build will be slow (compiles whisper.cpp from source, even without CUDA). Subsequent builds use the cargo cache.

If the build fails:
- Check whether `cmake` is on `PATH` (whisper-rs's build.rs uses it).
- Check whether the host C++ compiler is recent enough for C++17.
- Check whisper-rs's docs for any platform-specific prerequisites (Linux usually needs `build-essential`).

If failures are platform-specific and unfixable on the dev machine, document in a code comment that the local build is CPU-only and the canonical build runs on the A10 workspace. Do not block on local CUDA.

- [ ] **Step 4: Run the existing test suite to verify nothing regressed**

Run:
```bash
cargo test --features test-helpers
```

Expected: all existing Plan A tests pass (76 + 1 ignored as of session start). The new dependencies should not affect existing tests.

- [ ] **Step 5: Record the pinned versions in AD0009**

Edit `docs/decisions/AD0009-*.md`. In the decision text (the "we decided for X because" paragraph), insert the exact pinned values:

```
Pinned versions (as of YYYY-MM-DD):
- whisper-rs crate version: =0.13.X
- whisper.cpp commit tracked: <commit-sha or tag>
```

Add a one-line note about the upgrade discipline: "Bump these together (both lines) when whisper-rs releases a new version; verify the bake measurements still match the prior numbers before merging the bump."

- [ ] **Step 6: Run cargo fmt and clippy**

Run:
```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
```

Expected: no changes from fmt; clippy clean.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock docs/decisions/AD0009-*.md
git commit -m "$(cat <<'EOF'
feat(deps): add whisper-rs (cuda feature-gated) and hound

Adds the two dependencies Plan B Epic 1 needs:

- whisper-rs (pinned exact version, cuda feature gated) — embedded
  whisper.cpp via the recommended out-of-tree Rust binding.
- hound — pure-Rust WAV decoder for the float32 PCM 16kHz mono
  audio whisper.cpp expects.

The cuda feature is opt-in so local CPU builds still work; the SRC A10
workspace builds with --features cuda. Plan A's existing test suite
still passes unchanged.

AD0009 amended to record the exact pinned whisper-rs crate version
and the whisper.cpp commit it tracks; upgrade discipline added.

Refs: AD0009, AD0014

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Self-check

- [ ] `cargo build` succeeds on the dev machine (no cuda feature)
- [ ] `cargo build --features cuda` is documented as the A10 workspace build (don't try locally if CUDA toolkit isn't present)
- [ ] Existing tests pass with `cargo test --features test-helpers`
- [ ] AD0009 records the exact pinned versions
- [ ] Cargo.toml uses `=` prefix on whisper-rs version (exact pin)
