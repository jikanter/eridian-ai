---
name: Cite all sources in code comments
description: User requires protocol references, links, and rationale inline in comments for any code changes
type: feedback
---

Always cite sources (protocol docs, RFC links, OTP source references) and rationale inline in code comments when making changes.

**Why:** User wants to understand the provenance of every change and be able to verify against upstream documentation. Code should be self-documenting with respect to protocol specifications.

**How to apply:** When editing any code with external specifications, include links to the relevant documentation, the specific section/table being referenced, and the reasoning for each value or format choice. Don't just write the code — annotate it.
