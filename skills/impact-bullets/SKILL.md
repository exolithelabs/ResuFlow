---
name: impact-bullets
description: Rewrite experience bullets so each one shows a result, its scale, and how it was achieved.
---

# Impact bullets

Shape: **Verb + what you did + measurable result**, then *how* only if it adds credibility.

- Good: "Cut p99 checkout latency 41% (820→480 ms) by moving pricing to a read-through cache."
- Weak: "Responsible for improving performance of the checkout service."

Rules:

- One line, two at most (about 12–28 words). One idea per bullet.
- Start with a strong past-tense verb (Led, Shipped, Cut, Grew, Designed, Automated, Migrated). Current role: present tense is fine, but stay consistent.
- Quantify with what is true: %, time, money, users, requests/s, team size, frequency. If there is no number, give scope instead ("across 14 services", "for 3 regions"). Ask the user for real figures. Never fabricate a metric.
- Put the posting's keywords where they are honestly true.
- 3–5 bullets for recent roles, 1–2 for older ones. Order them by relevance to the target.
- No "I", "my", "responsible for", "helped", "worked on".

Workflow: `read_resume` → rewrite → apply with `edit_resume` (one bullet or section at a time, passing `expected_hash`) → run `check_resume` and fix what it flags.
