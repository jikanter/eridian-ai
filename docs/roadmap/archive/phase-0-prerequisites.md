# Phase 0: Prerequisites

**Status:** Done
**Commit(s):** `dde1078`

---

| Item | Status | Commit | Notes |
|---|---|---|---|
| 0A. Tool count warning (>20 tools) | Done | `dde1078` | Warns user, logs at debug level |
| 0B. Pipeline tool-calling (`call_react` in `pipe.rs`) | Done | `dde1078` | Stages with `use_tools` route through the agent loop |
| 0C. Pipeline config isolation | Done | `dde1078` | Model save/restore per stage, no shared-state mutation |
