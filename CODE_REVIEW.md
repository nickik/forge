# Forge Code Review

Review every compiler change for the following.

## Semantics

- Does behavior match the normative spec?
- Has accepted source syntax changed accidentally?
- Are evaluation order, overflow, bounds, `Option`, `Result`, and `unsafe` rules preserved?
- Is target-specific behavior clearly separated from language-defined behavior?

## Parser

- Is the grammar unambiguous for the new construct?
- Does error recovery remain deterministic?
- Are malformed forms tested?
- Could the change make a future compatible extension unnecessarily difficult?

## Type system

- Are implicit conversions being introduced accidentally?
- Are distinct types, enums, pointers, references, slices, arrays, `Option`, and `Result` kept semantically separate?
- Are refutable patterns rejected in irrefutable binding positions?

## Memory and safety

- Does durable allocation identify its allocator/owner?
- Does scratch memory escape its valid scope?
- Are raw pointer operations fenced by `unsafe`?
- Are checked operations still checked by default?

## Compiler quality

- Are diagnostics stable and actionable?
- Is the implementation simpler than the problem requires rather than more complex?
- Are data structures deterministic where output matters?
- Are tests present at the narrowest useful level?

## Compatibility

- If this touches public syntax or semantics, was `docs/compatibility.md` considered?
- If it changes a frozen v1 decision, is there an explicit language-decision update?
