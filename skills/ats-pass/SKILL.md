---
name: ats-pass
description: Make a resume parse cleanly in applicant tracking systems and match the target posting's keywords.
---

# ATS pass

1. Run `check_resume`. Fix every `error` and `warn` first; they are objective problems.
2. Structure that parses reliably:
   - Standard `##` section names: Summary, Experience, Skills, Education, Projects, Certifications.
   - Each role as `### Title — Company` followed by a `*Start – End · Location*` line.
   - Plain bullets. No tables, images, columns, icons or text in headers and footers.
   - Dates in one consistent format ("Jan 2022 – Present").
3. Keyword coverage: if the resume has a posting (`read_posting`), `check_resume` lists the posting's key terms that are `missing`. Compare those, and the target brief from `role-research`, against `read_instructions`. Each must-have keyword that is honestly true should appear at least once, ideally in Experience as well as Skills. Use the exact spelling and common acronym once: "Amazon Web Services (AWS)".
4. Spell out the Skills section as plain comma-separated lists grouped by type.
5. Contact details belong on the one contact line under the `# Name` at the top (`City · email · phone · links`). Plain text, no icons, and don't repeat them further down.
6. Re-run `check_resume`, then `render_resume` if you need to verify the final HTML.
