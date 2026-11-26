---
name: role-research
description: Turn a job posting into a targeting brief before writing or tailoring a resume.
---

# Role research

Use before creating or tailoring a resume for a specific job.

1. Get the posting text from the user (or fetch it if you can). Note company, title, level, location.
2. Extract, in the posting's own words:
   - **Must-haves**: hard requirements, years, certifications, stack.
   - **Signals**: repeated nouns and verbs, which reveal what the team actually cares about.
   - **Outcomes**: what success in the role looks like (scale, revenue, reliability, users).
3. Read the default template with `read_profile`, and the user's instructions from `get_workspace`. Follow every instruction. Map each must-have to concrete evidence from the user's history. Mark gaps honestly. Never invent experience to fill one.
4. Keep the targeting brief in the conversation: the top 5 keywords, the 3 strongest proof points, and the gaps. Do not write it into instructions; only the user edits those.
5. Create the variant with `create_resume` (copy the closest existing resume with `from`, or omit `from` to start from the default template) and tag it with the company. Save the posting text on it with `set_posting`, so `check_resume` reports which of its key terms the resume covers. Continue with the `impact-bullets` skill.

Keep keywords literal (ATS matching is mostly lexical): if the posting says "Kubernetes", don't write only "container orchestration".
