---
name: recruiter-review
description: Critique a resume the way a busy recruiter and a hiring manager would read it, with prioritised fixes.
---

# Recruiter review

Read the resume twice, as two people:

**Recruiter, 10 seconds.** From the header, summary and the first role only: is the target role obvious? Is the seniority clear? Is there one number that makes you keep reading?

**Hiring manager, 2 minutes.** Does the evidence prove the must-haves? Are claims specific and believable? Is there filler, jargon or an unexplained gap?

Report back as:

1. **Verdict**: one sentence, e.g. "Strong for senior backend roles; under-sells leadership."
2. **Top 3 fixes**, highest impact first, each with the exact line and a proposed rewrite.
3. **Cut list**: lines that don't earn their space for this target.
4. **Questions** for the user, about facts only they know (numbers, scope, outcomes).

Don't edit the file during a review unless the user asks. When they accept fixes, apply them with `edit_resume` and record lasting preferences ("prefers no summary section") with `append_memory`.
