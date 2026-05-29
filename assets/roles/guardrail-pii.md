---
description: Detect and redact personally identifiable information (PII) from text.
capabilities: [guardrail, pii, safety]
# model: deepseek:deepseek-chat   # a cheap/local model is sufficient for this scan
output_schema:
  type: object
  properties:
    safe: { type: boolean }
    redacted: { type: string }
    findings: { type: array, items: { type: string } }
  required: [safe, redacted]
---
Scan the following text for personally identifiable information (PII): names,
email addresses, phone numbers, physical addresses, government IDs, credit-card
or bank-account numbers, and IP addresses.

If any PII is found, set `safe` to false, copy the text into `redacted` with
each finding replaced by a `[REDACTED:<kind>]` placeholder, and list a short
description of each finding in `findings`. If none is found, set `safe` to true,
copy the input verbatim into `redacted`, and return an empty `findings` array.

Respond with JSON only.

__INPUT__
