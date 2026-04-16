# Plan A — Task 5: URL canonicalization (forms 1 and 2)

> Part of [Plan A: walking skeleton](./00-overview.md). See the overview for file structure, dependency set, conventions, and exit criteria.

**Files:**
- Create: `src/canonical.rs`
- Modify: `src/main.rs`
- Test: `tests/canonical.rs`

- [ ] **Step 1: Write the failing table test**

Create `tests/canonical.rs`:

```rust
use uu_tiktok::canonical::{canonicalize_url, Canonical};

#[test]
fn canonicalizes_form_1_tiktokv_share_video() {
    let result = canonicalize_url("https://www.tiktokv.com/share/video/7234567890123456789/");
    match result {
        Canonical::VideoId(id) => assert_eq!(id, "7234567890123456789"),
        other => panic!("expected VideoId, got {:?}", other),
    }
}

#[test]
fn canonicalizes_form_2_tiktok_user_video() {
    let result = canonicalize_url("https://www.tiktok.com/@coolcreator/video/7234567890123456789");
    match result {
        Canonical::VideoId(id) => assert_eq!(id, "7234567890123456789"),
        other => panic!("expected VideoId, got {:?}", other),
    }
}

#[test]
fn canonicalizes_form_1_with_query_string() {
    let result = canonicalize_url(
        "https://www.tiktokv.com/share/video/7234567890123456789/?utm_source=share",
    );
    match result {
        Canonical::VideoId(id) => assert_eq!(id, "7234567890123456789"),
        other => panic!("expected VideoId, got {:?}", other),
    }
}

#[test]
fn marks_short_link_form_3_as_needs_resolution() {
    let result = canonicalize_url("https://vm.tiktok.com/ZMabcdef/");
    match result {
        Canonical::NeedsResolution(url) => {
            assert_eq!(url, "https://vm.tiktok.com/ZMabcdef/");
        }
        other => panic!("expected NeedsResolution, got {:?}", other),
    }
}

#[test]
fn marks_short_link_form_4_as_needs_resolution() {
    let result = canonicalize_url("https://www.tiktok.com/t/ZTabcdef/");
    match result {
        Canonical::NeedsResolution(url) => {
            assert_eq!(url, "https://www.tiktok.com/t/ZTabcdef/");
        }
        other => panic!("expected NeedsResolution, got {:?}", other),
    }
}

#[test]
fn rejects_non_tiktok_url() {
    match canonicalize_url("https://example.com/video/123") {
        Canonical::Invalid(_) => {}
        other => panic!("expected Invalid, got {:?}", other),
    }
}

#[test]
fn rejects_malformed_url() {
    match canonicalize_url("not a url at all") {
        Canonical::Invalid(_) => {}
        other => panic!("expected Invalid, got {:?}", other),
    }
}
```

The integration test references the crate as `uu_tiktok` — the binary crate auto-exposes a library named after the package only if we have a `lib.rs`. Create `src/lib.rs` for this purpose:

Create `src/lib.rs`:
```rust
pub mod canonical;
```

(The library is purely for re-exporting modules to integration tests; the binary stays in `main.rs`.)

- [ ] **Step 2: Run the test to confirm it fails**

Run: `cargo test --test canonical 2>&1 | tail -15`
Expected: FAIL — `canonical` module does not exist.

- [ ] **Step 3: Implement `canonical.rs`**

Create `src/canonical.rs`:

```rust
use once_cell::sync::Lazy;
use regex::Regex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Canonical {
    /// URL parsed cleanly to a 19-digit numeric video_id (forms 1 and 2).
    VideoId(String),

    /// Short link (forms 3 and 4): `vm.tiktok.com/...` or `tiktok.com/t/...`.
    /// Cannot extract video_id without following the redirect. Plan C resolves
    /// these; Plan A logs and skips them.
    NeedsResolution(String),

    /// Not a TikTok URL or unparseable.
    Invalid(String),
}

// Form 1: https://www.tiktokv.com/share/video/{19-digit-id}/[?...]
// Form 2: https://www.tiktok.com/@username/video/{19-digit-id}[?...]
static CANONICAL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"^https?://(?:www\.)?(?:tiktokv|tiktok)\.com/(?:share/video|@[^/]+/video)/(\d{19})(?:/|\?|$)",
    )
    .expect("canonical regex compiles")
});

// Forms 3 and 4: short links that 302 to a canonical form.
static SHORT_LINK_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^https?://(?:vm\.tiktok\.com|vt\.tiktok\.com|(?:www\.)?tiktok\.com/t)/[A-Za-z0-9]+/?$")
        .expect("short-link regex compiles")
});

pub fn canonicalize_url(url: &str) -> Canonical {
    if let Some(captures) = CANONICAL_RE.captures(url) {
        let id = captures.get(1).expect("group 1 captured").as_str();
        return Canonical::VideoId(id.to_string());
    }
    if SHORT_LINK_RE.is_match(url) {
        return Canonical::NeedsResolution(url.to_string());
    }
    Canonical::Invalid(url.to_string())
}
```

- [ ] **Step 4: Run integration test to confirm pass**

Run: `cargo test --test canonical 2>&1 | tail -10`
Expected: `7 passed; 0 failed`.

- [ ] **Step 5: Wire `canonical` into the binary too**

Add `mod canonical;` to `src/main.rs` so the binary can use it later (the integration test goes through `lib.rs`; the binary needs the same module reachable internally — `main.rs` and `lib.rs` are separate compilation units).

Run: `cargo build 2>&1 | tail -3`
Expected: clean build.

- [ ] **Step 6: Commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings 2>&1 | tail -5
git add Cargo.lock src/canonical.rs src/lib.rs src/main.rs tests/canonical.rs
git commit -m "Plan A T5: canonical URL parsing for forms 1 and 2; short links flagged"
```
