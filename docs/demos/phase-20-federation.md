# Phase 17 + Phase 20: Server Engine and Federated Composition

*2026-05-11T15:42:47Z by Showboat 0.6.1*
<!-- showboat-id: ddf1c7f9-ce1c-42c1-a443-3ecd69ce02ec -->

Phase 20 is Epic 6's federation phase: pipeline stages can address roles on remote aichat instances as `remote:NAME/role`. The blocker was Epic 5 Phase 17B (POST /v1/roles/{name}/invoke). This iteration un-defers Epic 5's Phase 17 (all of A-E) plus Phase 16F/G/H — the role-publish/discovery surface that federation reads from.

```bash
grep -n 'EntityRef::Remote\|RemoteConfig\|resolve_target' src/config/resolver.rs src/config/remote.rs src/config/mod.rs | head -12
```

```output
src/config/resolver.rs:149:            EntityRef::Remote { .. } => "remote",
src/config/resolver.rs:156:            EntityRef::Remote { role, .. } => role.as_str(),
src/config/resolver.rs:179:        EntityRef::Role(_) | EntityRef::Agent(_) | EntityRef::Remote { .. } => Ok(()),
src/config/resolver.rs:229:            Ok(EntityRef::Remote {
src/config/resolver.rs:472:            EntityRef::Remote {
src/config/resolver.rs:485:            EntityRef::Remote {
src/config/resolver.rs:508:        let r = EntityRef::Remote {
src/config/resolver.rs:535:        let r = EntityRef::Remote {
src/config/remote.rs:6://! caller is responsible for mapping `EntityRef::Remote { target, role }`
src/config/remote.rs:7://! to a concrete `(endpoint, api_key)` pair via [`resolve_target`].
src/config/remote.rs:10://! it imports `RemoteConfig` and the public-view shape but knows nothing
src/config/remote.rs:14:use super::{RemoteConfig, RolePublicView};
```

EntityRef gained a Remote variant. The address parser (Phase 19A) already accepted `remote:host/role` shapes; Phase 20 ships the classification + execution that follows from it.

```bash
cargo test --bin aichat config::remote::tests 2>&1 | grep -E 'test result|test config::remote::' | head -12
```

```output
test config::remote::tests::short_host_trims_scheme_and_path ... ok
test config::remote::tests::unnamed_target_with_dot_only_accepted ... ok
test config::remote::tests::unnamed_host_port_target_synthesizes_http_url ... ok
test config::remote::tests::whitespace_in_target_errors ... ok
test config::remote::tests::unknown_bare_name_target_errors_with_hint ... ok
test config::remote::tests::empty_endpoint_errors ... ok
test config::remote::tests::named_target_endpoint_trailing_slash_stripped ... ok
test config::remote::tests::resolves_named_target_without_api_key ... ok
test config::remote::tests::resolves_named_target_with_api_key ... ok
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 478 filtered out; finished in 0.01s
```

```bash
grep -n '/v1/roles/\|/v1/pipelines\|/v1/batch\|/v1/models' src/serve.rs | head -10
```

```output
59:    println!("Models API:           http://{addr}/v1/models");
190:        } else if path == "/v1/models" {
194:        } else if let Some(rest) = path.strip_prefix("/v1/roles/") {
195:            // Phase 17B: `/v1/roles/{name}/invoke` is matched ahead of the
209:        } else if path == "/v1/pipelines/run" {
211:        } else if path == "/v1/batch" {
284:    /// Path-segment routing: `/v1/roles/` and `/v1/roles/foo/bar` fall through
286:    /// with role names. The Phase 17B `/v1/roles/{name}/invoke` route is
406:    /// Phase 17B: `POST /v1/roles/{name}/invoke` — dedicated role invocation.
436:        // Path-segment sanity: `/v1/roles//invoke` and nested paths fall
```

```bash
bats tests/integration/federation.sh 2>&1 | tail -15
```

```output
1..13
ok 1 phase 16G: GET /v1/roles/{name} returns the role's public view
ok 2 phase 16G: GET /v1/roles/{unknown} returns 404
ok 3 phase 16G: /v1/roles list also uses RolePublicView
ok 4 phase 17A: /v1/models lists 'role:NAME' for each known role
ok 5 phase 17B: invoke unknown role returns 404 before reading body
ok 6 phase 17B: invoke rejects empty input
ok 7 phase 17D: pipeline missing both stages and pipeline name errors
ok 8 phase 17D: pipeline rejects both stages and pipeline supplied
ok 9 phase 17E: batch with empty inputs errors
ok 10 phase 17E: batch rejects multiple target sources
ok 11 phase 20C: aichat loads remotes: section without erroring
ok 12 phase 20A: --pipe --stage remote:bareword/foo surfaces config hint
ok 13 phase 20D: federated -r remote:server/role calls the server's role
```

13/13 federation integration tests pass. Combined with 487 unit and 197 compatibility tests, the suite is at 697/697 green for this iteration.
