---
name: zed-crash-investigation
description: Investigate and fix Zed crashes via Sentry. Use when triaging, investigating, or fixing a Zed crash/panic reported in Sentry, or when the user asks to look into a crash, panic, or Sentry issue.
---

# Zed Crash Investigation

Workflow for triaging and fixing Zed crashes using Sentry and the Factory prompts.

## Fetch the crash

```
script/sentry-fetch <issue-id>
```

To pick a candidate to work on:

```
script/select-sentry-crash-candidates
```

## Follow the Factory prompts

The canonical workflow lives in these prompt files — load and follow them:

- `.factory/prompts/crash/investigate.md` — root-cause the crash.
- `.factory/prompts/crash/fix.md` — implement the fix.
- `.factory/prompts/crash/link-issues.md` — tie related issues together.

## Procedure

1. `script/sentry-fetch <issue-id>` (or pick a candidate via `select-sentry-crash-candidates`).
2. Load `.factory/prompts/crash/investigate.md` and follow it to isolate the root cause.
3. Load `.factory/prompts/crash/fix.md` and apply the fix.
4. Validate with the narrowest test / `./script/clippy` for the touched crate.
5. If relevant, link related issues per `link-issues.md`.

## Build

Heavy builds go to the remote compiler (see the `remote-compiler` skill), not the laptop.
