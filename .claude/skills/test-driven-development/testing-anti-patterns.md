# Testing Anti-Patterns & Rationalizations

**Load when:** writing or changing tests, adding mocks, adding test-only methods to production code, or tempted to skip TDD.

## Core principle

Test what the code does, not what the mocks do. Mocks isolate; they are not the thing under test.

## Iron laws

1. Never test mock behavior.
2. Never add test-only methods to production classes.
3. Never mock without understanding the dependency chain.

---

## Rationalizations for skipping TDD

All have the same answer: **stop, delete, restart with TDD.**

| Excuse | Reality |
|---|---|
| "Too simple to test" | Simple code breaks. Test takes 30 seconds. |
| "I'll test after" | Tests-after pass immediately — proves nothing. |
| "Tests-after achieve the same goal" | Tests-after answer "what does this do?" Tests-first answer "what should this do?" Different artifacts. |
| "Already manually tested" | Ad-hoc ≠ systematic. No record, can't re-run. |
| "Deleting X hours of work is wasteful" | Sunk cost. Unverified code is technical debt; keeping it is the waste. |
| "Keep as reference, write tests first" | You'll adapt it. That's testing-after. Delete means delete. |
| "Need to explore first" | Fine — throw the exploration away, then TDD. |
| "Test hard = design unclear" | Listen to the test. Hard to test = hard to use. |
| "TDD will slow me down" | Slower than what? Debugging-in-production is the alternative. |
| "Existing code has no tests" | You're improving it — add tests as you go. |
| "It's about spirit not ritual" | The ritual is what creates the proof. Skip it, lose the proof. |
| "This is different because…" | It isn't. |

---

## Anti-Pattern 1: Asserting on mock existence

```typescript
// BAD: verifies the mock works, not the component
expect(screen.getByTestId('sidebar-mock')).toBeInTheDocument();

// GOOD: test real behavior, or don't mock at all
expect(screen.getByRole('navigation')).toBeInTheDocument();
```

**Gate:** before asserting on any mock element, ask "am I testing real behavior or just mock existence?" If existence — delete the assertion or unmock the component.

---

## Anti-Pattern 2: Test-only methods in production classes

```typescript
// BAD: destroy() only called from tests, but lives on the production class
class Session { async destroy() { /* test cleanup */ } }

// GOOD: test utilities own test cleanup; Session has no destroy()
// In test-utils/
export async function cleanupSession(session) { /* ... */ }
```

Pollutes production API, risks accidental production calls, violates separation of concerns.

**Gate:** before adding a method to a production class, ask "is this only used by tests?" If yes — put it in test utilities.

---

## Anti-Pattern 3: Mocking without understanding dependencies

```typescript
// BAD: mocks the method whose side effect the test relied on
vi.mock('ToolCatalog', () => ({ discoverAndCacheTools: vi.fn() }));
// addServer's duplicate check depended on the config-writing side effect — now broken
```

Over-mocking "to be safe" silently breaks the behavior under test. The test either passes for the wrong reason or fails mysteriously.

**Gate:** before mocking, run the test against the real implementation first. Then mock at the lowest level that addresses the actual cost (slow I/O, network) — not the high-level method whose behavior the test depends on.

Red flags: "I'll mock this to be safe", "this might be slow, better mock it", any mock added without tracing the dependency chain.

---

## Anti-Pattern 4: Incomplete mock responses

```typescript
// BAD: only the fields you happened to know about
const mock = { status: 'success', data: { userId: '123' } };
// Breaks when downstream code reads response.metadata.requestId

// GOOD: mirror the full real response
const mock = {
  status: 'success',
  data: { userId: '123', name: 'Alice' },
  metadata: { requestId: 'req-789', timestamp: 1234567890 }
};
```

Partial mocks hide structural assumptions and fail silently when downstream code touches omitted fields. Test passes; integration breaks.

**Gate:** when mocking a response, examine an actual sample and include every field the system might consume. If uncertain, include all documented fields.

---

## Quick reference

| Anti-pattern | Fix |
|---|---|
| Assert on mock elements | Test real component, or unmock |
| Test-only methods on production class | Move to test utilities |
| Mock without understanding | Trace dependencies first; mock minimally at the lowest cost layer |
| Incomplete mock response | Mirror real API completely |
| Complex mock setup (>50% of test) | Consider integration test with real components |

## Bottom line

If TDD reveals you're testing mock behavior, you went wrong upstream. The fix is also upstream: write the test against real code, watch it fail, then add mocks only where isolation actually requires it.
