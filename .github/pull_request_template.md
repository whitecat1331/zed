# Objective

- Describe the objective or issue this PR addresses.
- If you're fixing a specific issue, use "Fixes #X" for each issue as [described in the GitHub docs](https://docs.github.com/en/issues/tracking-your-work-with-issues/using-issues/linking-a-pull-request-to-an-issue#linking-a-pull-request-to-an-issue-using-a-keyword).

## AI Disclosure

> Required for every PR (see [AI_POLICY.md](./AI_POLICY.md)). Do not delete.

- Primary author of the majority of this PR and its code (human, or agent + model):
- Model(s) used (or "None — no AI was used"):

## Solution

- Describe the solution used to achieve the objective above.

## Testing

- Did you test these changes? If so, how?
- Are there any parts that need more testing?
- How can other people (reviewers) test your changes? Is there anything specific they need to know?
- If relevant, what platforms did you test these changes on, and are there any important ones you can't test?

## Debugger harness evidence

> Required if AI was used (see [AI_POLICY.md](./AI_POLICY.md)). If no AI was used, write "N/A — no AI used".

- Acceptance report (dated `TEST_REPORT-*.md`):
- Loop state confirmed clean (`ISSUES.json` / `LOOP_STATE.json`):
- Build profile (`quick` / `release`) and the commit SHA the evidence was produced against:

## Self-Review Checklist:

- [ ] I've reviewed my own diff for quality, security, and reliability
- [ ] Unsafe blocks (if any) have justifying comments
- [ ] The content adheres to Zed's UI standards ([UX/UI](https://github.com/zed-industries/zed/blob/main/CONTRIBUTING.md#uiux-checklist) and [icon](https://github.com/zed-industries/zed/blob/main/crates/icons/README.md) guidelines)
- [ ] Tests cover the new/changed behavior
- [ ] Performance impact has been considered and is acceptable

## Showcase

> This section is optional. If this PR does not include a visual change or does not add a new user-facing feature, you can delete this section.

- Help others understand the result of this PR by showcasing your awesome work!
- If this PR includes a visual change, consider adding a screenshot, GIF, or video
  - A before/after comparison is very useful for changes to existing features!

While a showcase should aim to be brief and digestible, you can use a toggleable section to save space on longer showcases:

<details>
  <summary>Click to view showcase</summary>

My super cool demos here

</details>

---

Release Notes:

- N/A or Added/Fixed/Improved ...
