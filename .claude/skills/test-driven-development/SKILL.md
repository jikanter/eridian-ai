---
name: test-driven-development
description: Use when implementing any feature or bugfix, before writing implementation code. Loads TDD discipline — red-green-refactor, no code before a failing test.
---

# Test-Driven Development

## Overview

Write the test first. Watch it fail. Write minimal code to pass.

**If you didn't watch the test fail, you don't know if it tests the right thing.**

## When to Use

**Always:** new features, bug fixes, refactoring, behavior changes.

**Exceptions** (ask the user first): throwaway prototypes, generated code, configuration.

## The Iron Law

```
NO PRODUCTION CODE WITHOUT A FAILING TEST FIRST
```

Wrote code before a test? Delete it. Implement fresh from tests. No "keep as reference" — you'll adapt it, and that's testing-after.

## Red-Green-Refactor

### RED — Write one failing test

One behavior. Clear name. Real code (no mocks unless unavoidable).

```typescript
test('retries failed operations 3 times', async () => {
  let attempts = 0;
  const operation = () => {
    attempts++;
    if (attempts < 3) throw new Error('fail');
    return 'success';
  };

  const result = await retryOperation(operation);
  expect(result).toBe('success');
  expect(attempts).toBe(3);
});
```

### Verify RED — Run it. Mandatory.

Confirm:
- Fails (not errors out)
- Fails for the expected reason (feature missing, not typo)

Passes immediately? You're testing existing behavior — fix the test.

### GREEN — Minimal code to pass

```typescript
async function retryOperation<T>(fn: () => Promise<T>): Promise<T> {
  for (let i = 0; i < 3; i++) {
    try { return await fn(); }
    catch (e) { if (i === 2) throw e; }
  }
  throw new Error('unreachable');
}
```

No options, no extras, no refactoring of other code. Just pass the test.

### Verify GREEN — Run it. Mandatory.

Confirm: target test passes, other tests still pass, output is pristine (no warnings, no stray logs).

### REFACTOR — Clean up while green

Remove duplication, improve names, extract helpers. Don't add behavior. Tests stay green.

### Repeat

Next failing test for the next behavior.

## Good Tests

| Quality | Rule |
|---|---|
| Minimal | One thing. "and" in the name? Split it. |
| Clear | Name describes behavior, not mechanism. |
| Intent-revealing | Demonstrates the desired API. |

## When Stuck

| Problem | Solution |
|---|---|
| Don't know how to test | Write the wished-for API. Write the assertion first. Ask the user. |
| Test too complicated | Design too complicated. Simplify the interface. |
| Must mock everything | Code too coupled. Use dependency injection. |
| Test setup huge | Extract helpers, or simplify the design. |

## Debugging

Bug found? Write a failing test that reproduces it, then TDD the fix. Never patch without a test — the test is what prevents the regression.

## Verification Gate

Before marking work complete, all four must be true:

1. Every behavior has a test that failed first.
2. All tests pass; output is pristine.
3. Tests use real code (mocks only when unavoidable).
4. Edge cases and error paths are covered.

Any "no" → you skipped TDD. Start over.

## Tempted to skip, or adding mocks?

Read @testing-anti-patterns.md. It covers the common rationalizations ("too simple to test", "I'll test after", "tests-after achieve the same goals", "already manually tested", "deleting X hours is wasteful") and the four mock-related anti-patterns. Any rationalization means: stop, delete, restart with TDD.
