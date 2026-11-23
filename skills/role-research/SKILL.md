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
3. Read `profile.md` with `read_profile`. Map each must-have to concrete evidence from the user's history. Mark gaps honestly. Never invent experience to fill one.
4. Save a short brief with `append_memory`, headed `## Target: <Company> — <Title>`: the top 5 keywords, the 3 strongest proof points, and the gaps.
5. Create the variant with `create_resume` (copy the closest existing resume with `from`), tag it with the company, and continue with the `impact-bullets` skill.

Keep keywords literal (ATS matching is mostly lexical): if the posting says "Kubernetes", don't write only "container orchestration".
