---
name: feedback-commit-no-coauthor
description: "Never include \"Co-Authored-By: Claude\" trailer in git commit messages"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 996878d5-58ae-4a3f-91f2-d5bc72af6e98
---

Do not append `Co-Authored-By: Claude ...` (or any Claude co-author trailer) to commit messages.

**Why:** Explicit user preference — they don't want Claude attribution in their git history.

**How to apply:** When creating commits in any repo for this user, omit the Co-Authored-By trailer entirely from the commit message. Write the message and stop — no trailing attribution block.
