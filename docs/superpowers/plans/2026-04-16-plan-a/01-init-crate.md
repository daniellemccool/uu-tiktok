# Plan A — Task 1: Initialize crate with chosen dependencies

> Part of [Plan A: walking skeleton](./00-overview.md). See the overview for file structure, dependency set, conventions, and exit criteria.

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`

- [ ] **Step 1: Initialize the cargo project**

Run:
```bash
cd /home/dmm/src/uu-tiktok
cargo init --bin --name uu-tiktok
```

Expected: creates `Cargo.toml` and `src/main.rs` with a hello-world default. Working directory must be the repo root (`/home/dmm/src/uu-tiktok`).

- [ ] **Step 2: Replace `Cargo.toml` with the Plan A dependency set**

Overwrite `Cargo.toml` with:

```toml
[package]
name = "uu-tiktok"
version = "0.1.0"
edition = "2021"

[dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread", "process", "time", "fs", "io-util"] }
rusqlite = { version = "0.31", features = ["bundled"] }
clap = { version = "4", features = ["derive", "env"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = { version = "0.4", default-features = false, features = ["std", "clock", "serde"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
async-trait = "0.1"
anyhow = "1"
thiserror = "1"
regex = "1"
once_cell = "1"

[dev-dependencies]
tempfile = "3"
assert_cmd = "2"
predicates = "3"
```

- [ ] **Step 3: Replace `src/main.rs` with a hello-world that initializes tracing**

```rust
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!("uu-tiktok skeleton up");
    Ok(())
}
```

- [ ] **Step 4: Verify build and test scaffolding work**

Run:
```bash
cargo build 2>&1 | tail -3
cargo test 2>&1 | tail -3
```

Expected: build succeeds (may take a few minutes the first time as rusqlite compiles SQLite). `cargo test` reports `0 passed; 0 failed`.

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add Cargo.toml Cargo.lock src/main.rs
git commit -m "Plan A T1: initialize Rust crate with Plan A dependencies"
```
