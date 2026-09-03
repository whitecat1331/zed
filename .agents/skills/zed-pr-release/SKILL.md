---
name: zed-pr-release
description: Open or update a pull request in the Zed repo following Zed's PR hygiene rules (title, crate prefix, Release Notes section). Use when creating or updating a PR for the Zed fork.
---

# Zed PR Hygiene

Rules for opening/updating PRs in the Zed repo (from `.rules`).

## Title rules

- Clear, correctly capitalized, imperative mood. Example: `Fix crash in project panel`.
- No conventional-commit prefixes (`fix:`, `feat:`, `docs:`, ...).
- No trailing punctuation.
- Optional crate-name prefix when one crate is the clear scope: `git_ui: Add history view`.

Do not reuse a raw commit subject verbatim; derive a human-readable title.

## Body — Release Notes

The final section must be `Release Notes:`, with a blank line after the heading and exactly one bullet:

```
Release Notes:

- Added ...
```

Use exactly one of:

- `- Added ...`, `- Fixed ...`, `- Improved ...` for user-facing changes, or
- `- N/A` for docs-only / non-user-facing changes.

## Procedure

1. Commit work via `atomic-commits` (if applicable).
2. `gh pr create` (or `gh pr edit` for an existing PR).
3. Title per the rules above.
4. Body ends with the `Release Notes:` section.

## Before submitting

Remember the Zed HARD RULE: if you modified source files, the first two lines of `README.md` must be the `> [!IMPORTANT]` / `> Remove this line to confirm you've reviewed this PR before submitting.` banner. Never remove it yourself — that is the human author's review step.
