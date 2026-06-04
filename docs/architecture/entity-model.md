# The Entity Model — aichat's Foundational Building Block

**Status:** Accepted (foundational design) · **Owner:** aichat · **Created:** 2026-06-04
**Realized by:** Epic 10 (Entity Evolution), [Phase 52](../roadmap/phase-52-overview.md) ·
**Relates to:** the trace keystone ([`SPEC-001`](../analysis/caching/SPEC-001-trace-format.md))

> This document defines the **Entity** as the foundational unit of aichat, and a **facet
> taxonomy** that explains the existing Prompt / Role / Agent / Macro types as *presets* over one
> substrate rather than four unrelated things. It is **future-looking**: it records the decisions
> we are committing to now (§9) *and* keeps the one load-bearing question — whether to let facets
> and backings compose freely — explicitly reopenable.

---

## 1. Thesis

aichat has accreted four "entity types" — **Prompt**, **Role**, **Agent**, **Macro** — each
documented as its own thing with its own capability list. That framing hides the real structure:
they are four points in one space, and the runtime already treats them uniformly through a single
trait (`RoleLike`, `src/config/role.rs:182`). Everything resolves to a `Role` and runs through
`call_react`.

The foundational building block is therefore **neither Role nor Agent**. It is the **Entity**: a
*named, addressable, invocable, traceable* configuration that produces LLM calls. Role and Agent
are two **presets** over it. This document makes that latent abstraction explicit and gives it a
taxonomy precise enough to reason about — and to extend — without merging any structs.

## 2. Two keystones — the symmetry that bounds the design

The roadmap already names the **trace** ([`SPEC-001`](../analysis/caching/SPEC-001-trace-format.md))
as the keystone every *runtime* consumer reads. The Entity is the symmetric idea on the
*authoring* side:

| | **Entity** (authoring keystone) | **Trace** (runtime keystone) |
|---|---|---|
| Is | the noun every authoring surface *produces* | the record every invocation *produces* |
| Owns | identity, capability, resolution | what actually happened |
| Analogy | a *class* | an *instance* |

They meet at exactly one verb: **resolve Entity → execute → emit Trace.** This bounds the design.
If a proposed Entity feature is really about *what happened at runtime* (cost, latency, which tool
was called), it belongs in the trace, not the Entity. The Entity is static identity + declared
capability; the trace is the dynamic record of a single invocation of it.

Every authoring surface in the ecosystem produces an Entity by some backing:

- **aichat** authors one as a role **file**.
- **llm-functions** authors one as a tool-bearing **directory** (an agent).
- **brief** (format-first, never executes) can **emit** one declaratively.
- a remote host / MCP server **exposes** one by address.

One concept, many backings. That is the unification.

## 3. The litmus test — what is *core*

> **A property is *core* if you cannot resolve → invoke → trace a single LLM call without it.
> Everything else is a facet.**

Run the test and the core shrinks to almost nothing — which is the point. The core is the tax
*every* entity pays, including the cheap one-shot batch call that the project's
cost-conscious / batch-first constraints exist to protect.

### Core — the irreducible invocable

| Core property | Why it cannot be a facet | Today's fields |
|---|---|---|
| **Reference** | Without an address you cannot resolve it or attribute it in the trace | `name`, remote address (Epic 6) |
| **Identity envelope** | Discovery/introspection metadata; cheap, always present | `description`, `version`, `capabilities` tags (14A) |
| **Instruction source** | A call needs *a* system prompt (even empty) | `prompt` / `instructions` |
| **Model selection + generation params** | A call needs exactly one model and its knobs | `model_id`, `temperature`, `top_p` |

`aichat "summarize this"` and `aichat -r translator` are entities with **only** core + a static
instruction string. No schema, no tools, no state. The simple path stays free.

## 4. Facets — the six families

Everything beyond core is a **facet**: an optional, composable capability. Facets are **coarse** —
each is a *family*, and its internal sub-structure is an implementation detail, not a separate
facet. Facets cluster by the *question* they answer about an entity:

| Family (verb) | The question | Members (existing fields) |
|---|---|---|
| **Know** (context) | What does it know beyond the prompt? | Retrieval/RAG (`rag`, `knowledge_bindings`, `knowledge_mode`), Memory (`memory.jsonl`, 29B) |
| **Act** (actuation) | What can it *do*? | Tools (`functions.json`, agent-as-tool, `use_tools`), MCP servers (`*_mcp_servers`, 6C) |
| **Shape** (contract) | What is its I/O shape? | Typed I/O (`input_schema`/`output_schema`/`examples`/`variables`), lifecycle hooks (`pipe_to`/`save_to`), attribution (`attributed_output`, 27D) |
| **Govern** (control) | How does its loop behave? | Policies (`fallback_models`, `schema_retries`, `stage_retries`, cost guards, `ReactPolicy` 29A), react config (`react_max_steps`, 28B), dynamic instructions (computed prompt, 29B) |
| **Compose** (structure) | What is it made *of*? | Inheritance (`extends`/`include`), pipeline (Phase 21), macro steps |
| **Judge** (evaluation) | How is it scored? | Metrics (`metrics`, 23A) |

**Guardrails are not a seventh family.** A "guardrail" is the union of **Shape** (schema
validation, preventive) + **Judge** (metrics, post-hoc) + **Govern** (policy halts). Naming it
separately would double-count. (Decision §9.1.)

## 5. The two governing principles

### 5.1 Closed taxonomy, open vocabularies

The set of facet **families is closed.** You cannot invent a new *kind* of facet the runtime and
the trace do not understand — that is precisely the framework-bloat that obscures prompts and
breaks trace attribution. But **within a family the vocabulary is open**: any number of tools, any
MCP server, any shell metric, and (per 29A) a `ReactPolicy` *trait* with pluggable
implementations. Extensibility lives *inside* known categories, never by adding categories.

### 5.2 Backing gates *ownership*, not *reference* — and this derives the preset ladder

Sort the facets by the storage they require:

- Facets needing **executable code or mutable state** require **directory** backing: owned tools
  (`functions.json`), memory (writable `memory.jsonl`), a documents corpus + embedding index
  (RAG), dynamic instructions (an executable `_instructions` function).
- Facets that are **declarative and stateless** fit in a **single file**: pipeline,
  `extends`/`include`, metrics, typed I/O, lifecycle hooks.

Now the historical split falls out as a **theorem, not an accident**: Agent's facets are exactly
the directory-needing ones; Role's facets are exactly the file-fitting ones. The Prompt < Role <
Agent ladder is *backing-determined* — it was simply never named as such.

The refinement that makes it airtight: **backing gates *ownership*, not *reference*.** A file-role
cannot *own* a toolset, but it can `use_tools` (reference one); it cannot own a corpus, but
`knowledge_bindings` *reference* an external KB; it cannot host an MCP server, but
`role_mcp_servers` *point at* one. References are backing-independent; only **ownership** of
executable/stateful facets needs a directory.

| Backing | Can *own* | Can only *reference* |
|---|---|---|
| **Ephemeral** (prompt) | nothing — it is core + a literal prompt | — |
| **File** (role) | Compose · Shape · Judge facets | tools · KB · MCP |
| **Directory** (agent) | everything (tools · memory · RAG · dynamic instructions) | — |
| **Remote** (Epic 6/20) | nothing locally — owned on the far side | the whole entity, by address |
| **brief-emitted** | declarative facets only (brief never executes) | tools / cassettes by reference |
| **MCP-exposed** | n/a — a reference target, not an authored entity | — |

## 6. The presets — Prompt, Role, Agent, Macro

A preset is a *blessed* `(backing, facet-set)` tuple with a name and an authoring format. The four
existing types are exactly four presets:

| Preset | Backing | Core | Owns facets | Notes |
|---|---|---|---|---|
| **Prompt** | ephemeral (`%%` temp role) | yes | none | `aichat "text"`. No file, no metadata, no persistence. |
| **Role** | file (`roles/name.md`) | yes | Compose · Shape · Judge; *references* Act/Know | `aichat -r name`. The declarative preset. |
| **Agent** | directory (`agents/name/`) | yes | Know · Act · Govern (owned) | `aichat -a name`. Defined in llm-functions. |
| **Macro** | file (`macros/name.yaml`) | — (orchestrator) | Compose only | `aichat --macro name`. Composes *other* entities in an isolated config clone; not itself invocable as one LLM call. |

Macro is the **orchestration preset** — it answers only the *Compose* question and delegates the
rest to the entities it sequences. This is the same seam as the "pipeline-as-Entity" question
(left open in §9.4): a pipeline and a macro are both "an entity whose execution is *invoke these
sub-entities*."

> **Today, Role and Agent are the only two presets that own facets, and they own *disjoint* sets.**
> "A role with memory" or "an agent with a pipeline" are *expressible* in this model but not yet
> *blessed*. Whether to bless them is the one deliberately-open question (§9.4).

## 7. Facet couplings (documented, not policed)

Facets are *mostly* orthogonal, but a few have real dependencies. We **document** them and move on
rather than building a dependency engine:

- **Attribution (Shape) needs a Know facet.** `attributed_output` (27D) is meaningless with
  nothing — RAG or memory — to attribute *to*.
- **Dynamic instructions (Govern) imply a resolution-time model call.** Computing the prompt via
  `_instructions` can cost tokens *before* the main turn; it is not free like a static prompt.
- **Owned facets imply a writable/executable backing (§5.2).** Memory needs a writable dir; tools
  and dynamic instructions need executable code.
- **Pipeline / macro (Compose) invoke sub-entities**, so their cost and trace fan out across the
  children, not just the parent.

These are advisory couplings surfaced in docs and `--dry-run`, not hard constraints in the type
system.

## 8. The `Entity` trait — formalization without a struct merge

The implementation move is **a rename and a widening, not a merge**:

- **`RoleLike` → `Entity`** (the trait). `Role` and `Agent` keep their separate structs and
  separate authoring contracts (file-in-aichat vs directory-in-llm-functions). The cross-repo
  boundary is preserved. `Agent::to_role()` stays the bridge; the runtime keeps speaking one
  trait, exactly as it does today (`src/pipe.rs:517`, `src/config/mod.rs:1030`).
- **Add capability introspection** to the trait: an entity can report which facet families it
  carries (and whether each is owned or referenced). This is what makes `--dry-run`, MCP
  capability negotiation, and uniform resolution work across presets without variant-specific
  branches.
- **Trace attribution falls out.** Each invocation's trace carries the `entity_id` *and the
  resolved facet set actually used*, which is exactly what Phase 49 (turn-to-fact attribution) and
  federation need. The Entity is the stable key the trace points back to.

**We do not** merge `Role` and `Agent` into one struct, add a new user-facing authoring format, or
open the full Cartesian product of (backing × facets). See §10.

## 9. Decisions & open questions

Recorded so future readers know what was settled and what was deliberately left movable.

1. **Guardrails are not a facet family** — they are Shape + Judge + Govern. *Settled.*
2. **Facets are coarse** — a family is the unit; sub-structure (e.g. input vs output schema) is
   internal. *Settled.*
3. **Couplings are documented, not enforced** by a dependency engine (§7). *Settled.*
4. **Off-diagonal presets stay latent — for now (the load-bearing, reopenable question).**
   We keep **Role and Agent as the only two facet-owning presets** and do *not* yet allow arbitrary
   `(backing, facet-set)` combinations (e.g. a file-role that owns memory, or an agent that owns a
   pipeline). This keeps the UX small and the cheap path cheap.
   **This is a deferral, not a rejection.** The taxonomy is explicitly built so the question can be
   reopened without rework: blessing a new preset is adding one `(backing, facet-set)` row, gated
   by the §5.2 ownership rule. **Revisit when** there is concrete, repeated demand for a specific
   off-diagonal combination — at which point bless *that one combination*, never the whole product.

## 10. What this is **not**

| Not | Why |
|---|---|
| A merge of the `Role` and `Agent` structs | `to_role()` works; the two authoring contracts (file vs llm-functions directory) are deliberately independent and cross-repo. |
| A new user-facing authoring format | Users still write a role file or an agent directory. "Entity" is the internal spine + the explanatory model. |
| An open set of facet *families* | Closed taxonomy (§5.1). New *kinds* of capability would obscure prompts and break trace attribution. |
| Free composition of backing × facets | Deferred behind §9.4 — blessed one combination at a time, on demand. |
| A second telemetry/identity model | The trace stays the single runtime keystone; the Entity is the single authoring keystone (§2). |

## 11. Field-mapping appendix

Every current Role/Agent field, classified:

```
CORE        reference · description · version · capabilities-tags
            · instructions/prompt · model_id · temperature · top_p

KNOW        rag · knowledge_bindings · knowledge_mode · memory.jsonl
ACT         functions.json · agent-as-tool · use_tools · *_mcp_servers
SHAPE       input_schema · output_schema · examples · variables
            · pipe_to · save_to · attributed_output
GOVERN      fallback_models · schema_retries · stage_retries
            · pipeline_budget_usd · react_max_steps · ReactPolicy
            · dynamic_instructions
COMPOSE     extends · include · pipeline · macro steps
JUDGE       metrics
```

## 12. Grounding

- Trait + bridge in source: `src/config/role.rs:182` (`RoleLike`), `src/config/agent.rs:365`
  (`Agent::to_role()`), `src/pipe.rs:517` and `src/config/mod.rs:1030` (uniform dispatch).
- Architecture prose: [`architecture.md` → Entity Types](architecture.md#entity-types).
- Roadmap realization: [Phase 52 — Entity model formalization](../roadmap/phase-52-overview.md),
  building under Epic 10 ([`analysis/epic-10.md`](../analysis/epic-10.md)).
- Runtime keystone (the symmetry of §2): [`SPEC-001`](../analysis/caching/SPEC-001-trace-format.md),
  [`ADR-0001`](../analysis/caching/ADR-0001-trace-as-keystone.md).
- Boundary rule it must not cross: [`anti-roadmap.md`](../roadmap/anti-roadmap.md) (no struct
  merge; brief never gains runtime code).
</content>
</invoke>
