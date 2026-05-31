# Phase 20: Remote & Federated Composition : Overview - Epic 6

**Status (2026-05-11):** Shipped together with the Phase 17 server engine
(blocker un-deferred) and Phase 16G role retrieval. 28 new unit tests +
13 federation integration tests; full suite 487 unit + 197 compatibility
+ 13 federation pass.

| Item | Description | Status |
|---|---|---|
| 20A | Remote role resolution — `remote:host:port/role-name` (raw authority) and `remote:NAME/role` (named lookup in `remotes:`). `EntityRef::Remote { target, role }`. CLI `-r` and `--pipe --stage` both accept it. | **Done** |
| 20B | Remote role discovery — `GET /v1/roles/{name}` returns `RolePublicView`. Client side calls it via `remote::discover` (used at preflight time when wired). | **Done** |
| 20C | `remotes:` config section — `RemoteConfig { endpoint, api_key }` with `${VAR}` interpolation, documented in `config.example.yaml`. | **Done** |
| 20D | Federated pipeline execution — `resolve_stage_entity` dispatches `Remote` stages through `remote::invoke` (POST `/v1/roles/{name}/invoke`); metrics + trace carry through; preflight defers capability checks to the remote. | **Done** |

## 20A Design — Remote Resolution

```yaml
# config.yaml
remotes:
  staging:
    endpoint: http://staging.internal:8080
    api_key: ${STAGING_API_KEY}
  security:
    endpoint: http://security-scanner.internal:8080
```

```yaml
# roles/secure-review.md
pipeline:
  - role: extract                              # local
  - role: remote:security/vulnerability-scan   # remote aichat instance
  - role: summarize                            # local
```

Two address forms:
- `remote:<name>/<role>` — `<name>` looks up an entry in the `remotes:` table.
- `remote:<host[:port]>/<role>` — direct authority; resolver synthesizes `http://<host[:port]>` and no auth. Used for ad-hoc / local-network targets without permanent config.

A bare name that is not in `remotes:` and does not look host-shaped (no `.` and no `:`) errors at execution time with a hint to either add it under `remotes:` or use the host:port form.

## 20B Design — Discovery

`remote::discover(client, ResolvedRemote)` issues `GET /v1/roles/{name}` and parses the response into a `RolePublicView` (Phase 16F/16G). The server-side projection strips the prompt body, shell-injective defaults, and pipeline stage names; only the contract (schemas, capabilities, port summaries) is published. Discovery is currently used by tests and is available to consumers; it is not yet on the hot path of every invocation (deferred until federation usage motivates the latency cost).

## 20D Design — Federated Execution

`pipe::resolve_stage_entity` returns a new `StageTarget::{Local, Remote}` enum. For `Remote`, `run_stage_inner` issues a `POST /v1/roles/{name}/invoke` and folds the result into a normal `(String, CallMetrics)` so upstream stage retry / fallback machinery is unchanged. The remote's `usage.model` is annotated as `remote:<short-host>:<model>` so traces show where each stage ran.

## Implementation Notes

**Dependencies un-deferred:** Phase 17 (Epic 5 — Server Engine) shipped alongside this phase. The federation path requires Phase 17B's `POST /v1/roles/{name}/invoke`, and discovery requires Phase 16G's `GET /v1/roles/{name}`. Phase 17A/C/D/E also landed for symmetry.

**Files touched:**
- `src/config/remote.rs` *(new, ~340 lines incl. tests)* — `ResolvedRemote`, `resolve_target`, `discover`, `invoke`, 9 unit tests
- `src/config/mod.rs` — `RemoteConfig` struct + `remotes:` field on `Config`, env-var resolution, 7 new unit tests
- `src/config/resolver.rs` — `EntityRef::Remote { target, role }` variant, `classify_address` and `pipeline_stage_admissible` extended, 3 new tests
- `src/config/preflight.rs` — `validate_pipeline_stages` skips local model checks for `Remote` stages
- `src/pipe.rs` — `StageTarget::{Local, Remote}` enum, `run_stage_inner` dispatches remote stages over HTTP, public `invoke_role` / `invoke_role_streaming` / `run_inline_pipeline` / `load_pipeline_stages` / `InlineStage` / `StageTrace` / `InvokeResult` / `StageEvent` for Phase 17 endpoints
- `src/serve.rs` — Phase 16G + Phase 17A/B/C/D/E HTTP routes (`/v1/roles/{name}`, `/v1/roles/{name}/invoke` with optional streaming, `/v1/pipelines/run`, `/v1/batch`); `role:<name>` virtual models in `/v1/models`; `chat_completions_via_role` adapter
- `src/main.rs` — CLI `-r remote:NAME/role` dispatch through `run_inline_pipeline`
- `config.example.yaml` — `remotes:` documented next to MCP section
- `tests/integration/federation.sh` *(new)* — 13 end-to-end tests covering discovery, invoke, pipeline, batch, and federation routing

## Limitations Carried Forward

- Remote discovery results are not cached; every preflight against a known-remote pipeline incurs the round-trip cost. A short-lived in-memory cache is the natural follow-up.
- Streaming invocation through a remote stage is stage-granular, not token-granular. The remote runs to completion before the local pipeline sees its output.
- Auth is `Authorization: Bearer <key>` only. mTLS and signed-request schemes are future work.
- The CLI `-r remote:...` path runs as a single-stage pipeline and emits raw output to stdout; it doesn't yet surface the trace envelope the way `--pipe` does under `-o json`.
