# AI Development Policy

This repository is a **hard fork** of
[`zed-industries/zed`](https://github.com/zed-industries/zed). It no longer
tracks upstream `main` as a merge or rebase target; worthwhile upstream changes
are cherry-picked in. This policy governs how artificial-intelligence tooling
may be used to contribute to this fork.

## Repository disclosure

The majority of this fork was developed using **DeepSeek V4 Pro**, with
assistance from **Claude Sonnet 4.6** and **Gemini 3.1 Pro**. This disclosure
applies to the fork's own work, not only to external contributions.

## AI is welcome

Using AI — assistants, coding agents, or models — to write code, documentation,
or any other contribution **is allowed**, including work that is wholly
AI-generated. There is no blanket ban on autonomous agents.

That permission carries two **hard** obligations (A and B) and one **soft**
obligation (C). A PR that uses AI and does not satisfy A and B **will not be
considered**. A PR that also satisfies C is **reviewed first**.

## A. Disclose the primary author

Every PR must contain an **AI Disclosure** section stating, in plain terms:

- **Who or what wrote the majority of this PR and its code** — a human, or the
  agent and model(s) that produced the majority of the work.
- **Which model(s) were used.**

If no AI was used, say so explicitly ("No AI was used in this PR"). Do not
bury this in a footnote; it belongs near the top of the PR body.

## B. Prove the work with debugger-harness evidence

If AI was used, the PR must include evidence that the change was exercised
end-to-end — not merely that it compiles. The required evidence is a **full
acceptance run** through the Zed debugger harness (`zed-debugger-demo`), driven
by the `debugger-loop`:

- A completed, dated acceptance report (`test-reports/TEST_REPORT-*.md`) covering
  the full adapter matrix — every capability on every supported adapter, **not**
  a smoke run.
- The reconciled loop state (`ISSUES.json` / `LOOP_STATE.json`) for that run, so
  a reviewer can confirm the report corresponds to a real, clean, full run.
- A build produced at the PR's commit. Iteration PRs may use a `quick` profile;
  the merge PR (`dev` → `main`) must use a **full `--release` profile**
  (`build_profile = "release"`).

Attach the report to the PR and state the commit SHA the evidence was produced
against. The evidence must correspond to the **exact commit** the PR is based on.

## C. Show the root cause → fix → verification (soft)

This is **soft-required**: a PR that includes it is reviewed **first**; one that
omits it is not rejected, only deprioritized. It is the proof that the author
understands the change, not just that a script passed.

- **Bug fix:** state the **root cause** (found), the **fix** (what changed and
  why), and the **verification** (how you know it is correct — tests, harness
  evidence, or both).
- **Feature / refactor / docs:** state the **motivation**, the **approach**, and
  the **verification**. There is no "found" for a change that was not a bug.

Keep it proportional to the change — a one-line fix owes a one-line root cause
and a one-line verification, not an essay.

## If a hard rule is unmet

A PR that uses AI without a clear AI Disclosure, or without the required
debugger-harness evidence, will be **closed or left unreviewed**. A PR that omits
the root-cause narrative (C) is **not** rejected — it is simply reviewed after
PRs that include it. The bar is the evidence, not the tools that produced the
change.

## Rationale

Whether or not AI was used, **you (the human) own and are responsible for the
work in every PR you open.** Using a tool does not transfer accountability for
the change — if it is wrong, that is on the author, not the model.

AI disclosure is **not** about blame or accountability. It is a personal metric:
the maintainer uses it to understand which models are good at what, and to
compare agentic development to code written by hand. The acceptance evidence is
the part that proves the change actually works against the real debugger-tool
surface this fork exists to maintain.
