---
name: zed-issue-severity
description: Reference for Zed's GitHub issue severity labels (S0–S3) and their
  triage definitions. Use when triaging, labeling, or discussing Zed issues/PRs,
  or when someone asks what a `severity:S#` label means or whether a bug's
  severity looks correct.
---

# Zed Issue Severity Scale

Zed's issue tracker (`zed-industries/zed`) uses four `severity:` labels to
triage bugs. The scale is **S0–S3** — there is **no S4** (the label does not
exist; `GET /labels/severity%3AS4` returns 404).

| Label | Definition |
|-------|-----------|
| `severity:S0` | "Drop everything now" bugs: security holes with exploit, big data/money loss, users can't work |
| `severity:S1` | Security holes w/o exploit, crash, install/update, sign-in, badly broken big features |
| `severity:S2` | Average run-of-the-mill bugs; a feature is broken but only partially |
| `severity:S3` | Papercuts, minor issues with a clear non-tedious workaround, cosmetic bugs |

All four labels share the same color (`b60205`, red); the number is the only
visual differentiator.

## When to use

- Someone asks what `severity:S2` (or any `severity:S#`) means.
- You are triaging or labeling a Zed issue/PR and need to pick a severity.
- You suspect a label is mis-assigned (e.g. a full Windows hang tagged `S2`
  rather than `S1`) and want to flag it.

## How to verify / refresh

The authoritative source is the repo's label metadata. Fetch one label at a
time (the colon must be URL-encoded as `%3A`):

```
https://api.github.com/repos/zed-industries/zed/labels/severity%3AS0
https://api.github.com/repos/zed-industries/zed/labels/severity%3AS1
https://api.github.com/repos/zed-industries/zed/labels/severity%3AS2
https://api.github.com/repos/zed-industries/zed/labels/severity%3AS3
```

## Related triage labels

Severity is one of several independent triage axes. Others commonly seen on
the same issue include:

- `state:` — workflow state (e.g. `state:needs repro`, meaning the issue lacks
  reliable reproduction steps and may be parked until then).
- `reach:` — affected population (e.g. `reach:some users`, <⅓ of users).
- `area:` — subsystem (e.g. `area:ai/agent thread`).

Do not conflate `severity:` with these. A high-severity bug can still carry
`state:needs repro` if it has not been reliably reproduced.
