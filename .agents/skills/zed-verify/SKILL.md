---
name: zed-verify
description: Run Zed's local verification gates — clippy, tests, formatting, and repo checks — before committing or pushing. Use when the user asks to run clippy, run tests, check the build, or verify a change in the Zed repo.
---

# Zed Verify

Run the Zed repo's local checks. Use `./script/clippy`, not `cargo clippy` (it pins the right toolchain and flags).

## Clippy / lint

```
./script/clippy          # or ./script/clippy.ps1 on Windows
```

## Tests

Narrow first, then broader:

```
cargo test -p <crate> [<test_name>] -- --nocapture
```

For GPUI tests (seeds, iterations, parking failures), use the `gpui-test` skill.

## Repo checks

```
script/check-keymaps
script/check-licenses
script/check-links
script/check-todos
```

## Quick build check

Heavy builds go to the remote compiler (see `remote-compiler`). For a quick
local check on one crate:

```
cargo check -p <crate>
```

## Order of operations

1. `./script/clippy`
2. narrow `cargo test -p <crate>`
3. relevant repo checks
