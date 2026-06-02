# SPEC: Astrophage — the wire-level record/replay/cache/mock tool

**Status:** Draft, 2026-06-02
**Applies to:** aichat (this repo), [brief](https://github.com/jikanter/brief), and any future harness ([pi](https://pi.dev)) that points an OpenAI-compatible client at a `base_url`.
**In-repo contract:** [`SPEC-003-cache-substrate.md`](../../analysis/caching/SPEC-003-cache-substrate.md) (the wire behavior), [`ADR-0005-cache-substrate-extraction.md`](../../analysis/caching/ADR-0005-cache-substrate-extraction.md) (the extraction decision), [`EVAL-0005-build-vs-integrate-replay.md`](../../analysis/caching/EVAL-0005-build-vs-integrate-replay.md) (build-vs-buy), [`SPEC-004-ecosystem-surfaces.md`](../../analysis/caching/SPEC-004-ecosystem-surfaces.md) (per-repo surfaces).

## 0. Why this document exists

`ADR-0005` extracts Eridian's **wire-level response cache** into a separate binary
(working name `eridian-replay`). `SPEC-003` is its wire contract. Both live inside the
aichat repo and treat the binary as a **Cargo workspace member**.

This document is the **cross-repo projection** of that decision. It names the tool
**astrophage**, and it owns exactly the parts of the design that span **more than one
repo** — the seams between aichat, `brief`, and the harness — per the rule in
[`README.md`](README.md). It does **not** restate the wire contract; `SPEC-003` is
authoritative for canonicalization, the determinism gate, SSE synthesis, and the three
policies. When this doc and `SPEC-003` disagree on wire behavior, `SPEC-003` wins.

The single open structural question this doc settles that `ADR-0005` deliberately left
as a "working name": **astrophage is forked out of eridian into its own repo**, not kept
as a workspace member. §2 states the decision and its cost.

## 1. Name

**Astrophage** (from *Project Hail Mary*): a microorganism that **stores stellar energy**
and releases it on demand. The tool **stores model responses** — energy already paid for
in tokens — and releases them on a later request **without re-spending**. A cassette set
is a cultured strain: a pinned, committed colony of stored energy you can run an eval on
for free. The name is the function.

It also fits the courier/probe naming already in the integrated system (`beetle`,
*Project Hail Mary* couriers). Astrophage is the fuel; beetle carries the git coordination
between the repos that produce it.

## 2. Repo shape — the cross-repo decision

`ADR-0005` §Decision keeps the substrate as a workspace member "working name
`eridian-replay`". The `beetle` integrated-system set already lists `astrophage` as a
**peer repo** of `aichat` and `llm-functions`. This doc reconciles the two: **astrophage
is its own repo**, and the shared code is a **published-by-path crate** both repos depend
on.

```text
astrophage/                         # github.com/jikanter/astrophage (new repo)
├── Cargo.toml                      # bin: astrophage; lib dep: replay-core
└── src/{main.rs, gateway.rs, policy.rs, control.rs, trace.rs, cassette.rs}

replay-core/                        # shared crate (see §2.1 for where it lives)
└── src/{cas.rs, sse.rs, key.rs, lib.rs}

aichat/  (this repo)                # depends on replay-core for StageCache + serve path
```

The split is exactly `SPEC-003` §1 — `astrophage` is the `eridian-replay` binary,
`replay-core` is the shared CAS + SSE + canonical-key crate — with one change: the binary
is **not** in aichat's workspace. That is the only thing extraction-into-a-repo alters.

**Why a separate repo, not a workspace member.** A workspace member couples astrophage's
release cadence, CI, and issue tracker to aichat's. The substrate's whole value
(`EVAL-0005` §3) is being **runtime-agnostic** — a client other than aichat (a harness, a
promptfoo run, Claude Code) points `base_url` at it and gets cache/replay/mock with no
aichat dependency. A separate repo makes that independence structural rather than
aspirational, and lets a consumer vendor *just* astrophage without cloning aichat.

**The cost, stated plainly** (the `EVAL-0005` §5 counter-case, now realized): a second
repo is a second CI, a second release, a second README/demo surface for one maintainer.
`EVAL-0005` accepted that cost **only because eval-replay is committed**. If that
commitment is ever withdrawn, this repo should be re-absorbed as a workspace member or
dropped for a commodity gateway — the OpenAI-compat seam (§3) keeps that reversible.

### 2.1 Where `replay-core` lives `STOP-AND-ASK`

`replay-core` is depended on by **both** repos (aichat's `StageCache`/serve path *and*
astrophage). Three options, none free:

| Option | aichat → replay-core | astrophage → replay-core | Cost |
|---|---|---|---|
| **A. crate in astrophage repo** | path/git dep across repo | in-repo | aichat now build-depends on the astrophage repo — the dependency arrow points the wrong way (`base_url` is supposed to be the only coupling). |
| **B. crate in aichat repo** | in-repo | path/git dep across repo | astrophage build-depends on aichat — breaks the "vendor astrophage alone" goal. |
| **C. third repo / published crate** | dep | dep | a third repo to maintain; cleanest arrows. |

**Recommendation: C deferred, B for v0.1.** Ship `replay-core` inside aichat's workspace
first (it already refactors `src/cache.rs` onto it per `SPEC-003` §1, acceptance #2), and
have astrophage depend on it by **git tag**. Promote to a standalone published crate
(Option C) only if a third consumer needs it. This is flagged `STOP-AND-ASK` rather than
guessed because it sets the dependency topology of the whole integrated system —
surface it, do not assume it.

## 3. Seam: aichat ↔ astrophage

The coupling is **one URL and one header**. Nothing else.

- **`base_url`.** aichat points its OpenAI-compatible client at astrophage's listen
  address. Astrophage forwards misses upstream to the real provider. This is the entire
  data-plane coupling — identical to pointing at LiteLLM/Helicone, which is why it is
  reversible (`EVAL-0005` §6 rationale 4).
- **`X-Eridian-Session-Id` correlation header** (`SPEC-003` §6). aichat stamps every
  outbound request with the turn ULID; astrophage echoes it into every `SPEC-001`
  `cache.lookup` event it emits, so a cassette/cache hit correlates to the originating
  turn. Legitimate here because aichat owns the client (contrast `EVAL-001` §3's rejection
  for third-party MITM).
- **Mode selection** rides the existing `--cache-*` flag family and the
  `x-aichat-cache-mode` / `x-aichat-cassette` headers (`SPEC-003` §4, `SPEC-004` §Eridian).
  No new aichat architecture — these are the 38D control-protocol flags plus
  `--cache-mode` / `--cassette`.
- **Trace projection, not a second telemetry source** (`SPEC-003` §6). Astrophage emits
  through the same `SPEC-001` JSONL + blob contract. Accounting is *derivable from* the
  keystone trace; astrophage is a projection, never a parallel model (the F1
  anti-fragmentation rule).
- **Tool-replay stays Eridian-side** (`SPEC-004` §llm-functions). Astrophage caches the
  **wire** (LLM responses) only. Deterministic tool stdout is replayed by aichat's
  tool-dispatch layer from the keystone-trace blob store, keyed `(tool_name, args_hash)`.
  A deterministic eval = astrophage wire-replay **+** aichat tool-replay. Astrophage never
  sees or caches tool execution (`SPEC-004` §llm-functions; `SPEC-003` §0).

**What astrophage must never be taught** (`SPEC-003` §0, `ADR-0005` §1): the
structure-aware `StageCache` key `(role, model, input)`, provider prompt-cache
(`cache_control`) emission, or any knowledge `FactId`. Those are runtime-internal to
aichat. Pushing them across the seam re-imports runtime-awareness into the one component
whose value is being runtime-agnostic.

## 4. Seam: brief ↔ astrophage

This is the **integrated knowledge** the task asks for: how an intent author's `.brief.md`
binds a role to an astrophage cassette set, with **brief never running anything**.

`brief` is **format-first, synchronous, no `tokio`, no `reqwest`** — it declares and emits,
it does not execute (brief `CLAUDE.md` decisions 2–4;
[github.com/jikanter/brief](https://github.com/jikanter/brief)). So the seam is the **F6
pattern** (`SPEC-004` §brief): brief grows a *format field*, astrophage is the *runtime
consumer*, and the two never share a process.

**1. Author declares** a cassette binding next to the role's intent, either in a section:

```markdown
## Fixtures
- cassette: rust-reviewer-v1 — pinned eval-replay set for the rust-reviewer role
```

or in (extensible) frontmatter:

```yaml
cassettes: [rust-reviewer-v1]
```

**2. `brief emit` compiles** the binding to the eval harness's replay config — a promptfoo
`providerconfig` snippet that selects astrophage in replay mode:

```yaml
providers:
  - id: astrophage-replay
    config:
      command: |
        aichat --role {{role}} \
          --cache-mode cassette --cassette rust-reviewer-v1 --cache-replay \
          --trace-out /tmp/eridian-test-$$.jsonl {{prompt}}
```

brief emits a **string**; it neither records nor replays. astrophage (invoked by the
harness, with aichat as the client) does the work.

**3. astrophage records/replays** the named set per `SPEC-003` §7 (record → review →
commit → CI-replay; drift detection on key change).

**Honesty flag** (carried from `SPEC-004` §brief): the committed eval-replay story does
**not require** the brief field — a cassette can be named directly in
`promptfooconfig.yaml`. The `## Fixtures` / `cassettes:` field is a **convenience** for
teams who already author intent in `.brief.md` and want the cassette reference to live next
to the role it pins. It is therefore **deferred and optional**.

**Companion change, documented not applied** (cross-repo rule, `README.md`): adding
`## Fixtures` parsing + a `cassettes:` frontmatter field + emit support is a **`brief`-repo
task** for Jordan, in [github.com/jikanter/brief](https://github.com/jikanter/brief). It is
**never** edited from the aichat repo. If it is never built, brief stays untouched and the
direct-promptfoo path still works.

## 5. Seam: harness ↔ astrophage

The harness ([pi](https://pi.dev)) inherits cache/replay **by topology, for free**
(`SPEC-004` §pi). The pi bridge already runs against an in-process `aichat --serve`
(`src/repl/pi.rs`); when that server's client points at astrophage, pi turns are
cached/replayed transparently. No new harness architecture.

- **Mode select** via env at launch (`AICHAT_CACHE_MODE` / `AICHAT_CASSETTE`) or bridge
  headers (`x-aichat-cache-mode` / `x-aichat-cassette`).
- **Slash commands** extend the existing 37D set (`/cache-stats`, `/cache-clear`,
  `/transparent-cache on|off`) with `/cache-mode <mode>` and `/cassette <name>`, routed
  through `bridgeFetch`. No new pi-side surface beyond two commands.

## 6. The three policies (reference, contract in `SPEC-003` §3)

One CAS+SSE mechanism, three policies differing only in keying/eviction/faults:

| Policy | Keying | Writes | Faults | Cross-repo use |
|---|---|---|---|---|
| **cache** | canonical request key | auto on miss (TTL+LRU) | none | aichat/harness transparent cost reduction |
| **cassette** | canonical request key | only in *record* mode (pinned, no eviction) | none | brief/promptfoo eval-replay: deterministic, offline, token-free |
| **mock** | matcher (path/method/key/ordinal) | n/a | scripted status/latency/body/disconnect | cross-process fault injection for any downstream binary |

The canonical key is a runtime-agnostic SHA-256 over the request body's response-determining
fields (model, messages, system, sampling params, tools, response_format), with auth
headers / stream flag / correlation IDs / key-ordering normalized **out** (`SPEC-003` §2).
The same key means a streaming and non-streaming request hit the same stored entry.

## 7. Cassette portability (cross-repo artifact rule)

A cassette set is a **portable artifact**, like `mcp.json` (`SPEC-mcp-json-artifact.md`):
it lives **in the consuming repo**, not in aichat's or astrophage's source tree.

```text
<consumer-repo>/tests/regression/cassettes/<set-name>/
├── manifest.json            # set name, schema_version, ts, entry index
├── <canonical-key>.json     # redacted request + response + CallMetrics + stream flag
└── blobs/<sha256>           # large bodies, content-addressed (replay-core CAS)
```

- **Committed to the consumer**, replayed by astrophage. A `brief`-authored intent in
  repo X commits its cassettes under repo X; astrophage is the engine, never the home.
- **Redaction is a record-mode gate, not a later pass** (`SPEC-003` §6/§7). A committed
  cassette with a live `Authorization`/`X-Api-Key` is a security incident, so astrophage
  strips and pattern-scrubs (`SPEC-001` §6) before any byte hits disk. Acceptance #6: no
  cassette entry contains a plaintext key.
- **Drift is a feature** (`SPEC-003` §7). A role/prompt/model edit changes the canonical
  key; replay against the old set surfaces a miss-as-error naming the stale key —
  "the prompt changed but the eval passed against a stale recording" is caught, not
  silently passed. `astrophage cassette check` reports stale (unmatched pins) and uncovered
  (unpinned requests) for a set against a live request stream.

## 8. Non-goals (cross-repo)

Inherits `SPEC-003` §8 (no TLS MITM/CA, no rate-limiting, does not own structure-aware or
knowledge keys, does not replace in-process `wiremock-rs`, distributed backends deferred),
plus two that are specifically cross-repo:

- **No reverse dependency into a consumer.** astrophage must not build-depend on aichat,
  brief, or the harness. The only inbound coupling is `base_url` + the correlation header.
  (This is why §2.1 `replay-core` placement is `STOP-AND-ASK`.)
- **brief never gains runtime/network code.** The seam is format-only, forever. Any design
  that has brief invoke astrophage is out of scope by construction.

## 9. Open questions (`STOP-AND-ASK`, do not guess)

1. **`replay-core` home** (§2.1) — sets the dependency topology of the whole integrated
   system. Decide before astrophage's first cross-repo build.
2. **Tool-replay key stability** (`SPEC-004` §llm-functions stop-and-ask) — is
   `(tool_name, args_hash)` a stable *lookup* key across runs? `SPEC-001` §3.4 stores the
   hashes but does not promise lookup stability. If not guaranteed, the tool-replay key
   needs a schema decision before deterministic tool+wire eval works end-to-end.
3. **Canonical-key field edges** (`SPEC-003` §2) — any in-key/out-of-key field not already
   settled by 37C's `transparent_key` is a stop-and-ask, not a guess.

## 10. Acceptance criteria (cross-repo, additive to `SPEC-003` §9)

1. `astrophage` builds **outside aichat's workspace** and depends on `replay-core` by the
   decided mechanism (§2.1); a consumer can vendor astrophage without cloning aichat.
2. aichat ↔ astrophage coupling is exactly `base_url` + `X-Eridian-Session-Id`; removing
   astrophage (pointing `base_url` back at the provider) leaves aichat fully functional.
3. A cassette recorded against a live provider replays byte-identically offline with
   `cache_hit: true` and a `cache.lookup` event correlated to the turn `session_id`
   (`SPEC-003` §9.4), with **zero** aichat code in astrophage's dependency graph.
4. The `brief` companion change is **documented here and unbuilt in this repo**; the
   direct-promptfoo replay path works without it.
5. No cassette entry committed to any consumer repo contains a plaintext key
   (`SPEC-003` §9.6).
6. Drift: editing a brief-bound role's prompt changes the canonical key; CI replay against
   the committed set fails naming the stale entry rather than passing.

---

## Sources & cross-repo links

- In-repo contract: [`SPEC-003`](../../analysis/caching/SPEC-003-cache-substrate.md),
  [`SPEC-004`](../../analysis/caching/SPEC-004-ecosystem-surfaces.md),
  [`ADR-0005`](../../analysis/caching/ADR-0005-cache-substrate-extraction.md),
  [`EVAL-0005`](../../analysis/caching/EVAL-0005-build-vs-integrate-replay.md),
  [`SPEC-001`](../../analysis/caching/SPEC-001-trace-format.md).
- **brief** (companion change lives here, not in this repo): [github.com/jikanter/brief](https://github.com/jikanter/brief).
- **harness**: [pi](https://pi.dev) — inherits by topology (§5).
- Portability rule reused: [`SPEC-mcp-json-artifact.md`](SPEC-mcp-json-artifact.md),
  [`README.md`](README.md).
