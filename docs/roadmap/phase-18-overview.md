# Phase 18: Discovery & Estimation : Overview - Epic 5

> **[DEFERRED 2026-04-17]** Phases 16, 17, and 18 are parked while Epic 9
> (Knowledge Evolution) is in flight.

| Item | Description | Status |
|---|---|---|
| 18A | Cost estimation endpoint (`POST /v1/estimate` — token/cost preview without LLM call) | -- |
| 18B | OpenAPI specification (`GET /v1/openapi.json`) | -- |
| 18C | Root page (`GET /` — endpoint listing with links to spec) | -- |

**18A Design:** Returns estimated cost plus cheaper alternatives:

```json
{
  "estimated_cost_usd": 0.015,
  "alternatives": [
    {"model": "deepseek:deepseek-chat", "estimated_cost_usd": 0.0004},
    {"model": "openai:gpt-4o-mini", "estimated_cost_usd": 0.002}
  ]
}
```

## [Epic Details](./phase-18-server-discovery.md)
