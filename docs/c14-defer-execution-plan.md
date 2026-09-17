# C14 defer cleanup execution plan

## Goal

Close the remaining C14 `defer` executable matrix: deferred cleanup runs once,
in last-in-first-out order, on ordinary scope exit, early `return`, `break`,
`continue`, and `Result` `?` propagation.

## Normative basis

- `docs/forge-v1-spec.md` sections defining `defer`, loop control, `return`,
  and `Result` propagation;
- `docs/compatibility.md`, which freezes scope-exit cleanup order;
- `docs/compiler-architecture.md`, which requires typed-HIR decisions to be
  explicit in FIR before code generation.

## Current state

`FunctionLowerer` records deferred expressions and blocks per lexical cleanup
scope. It emits those scopes in reverse order for normal scope exit and for
the control-flow exits represented by `return`, `break`, and `continue`.
There is one native fixture covering ordinary return cleanup, but no single
executable proof covers nested ordering and the other non-local exits.

## Invariants

- A deferred action runs exactly once when its enclosing scope exits.
- Actions run in reverse declaration order.
- `break` and `continue` run only scopes exited by that transfer.
- `?` uses its typed propagation edge and therefore performs the same cleanup
  as an explicit early `return`.
- `return`, `break`, and `continue` inside a deferred cleanup remain rejected.

## Milestones

1. Add an AArch64 native fixture covering nested cleanup ordering, loop
   `continue`/`break`, and `?` propagation.
2. Add focused FIR assertions showing cleanup calls before each relevant
   terminator.
3. Run the focused frontend and native-spec gates, then the workspace gate.
4. Update the C14 matrix and top-level roadmap only when native evidence is
   green.

## Tests

- Native fixture returns a distinct non-zero status for each failed ordering
  or exit-path assertion.
- Frontend FIR test asserts cleanup calls precede return and loop-transfer
  terminators.
- Existing diagnostics keep rejecting control transfer from cleanup bodies.

## Risks / decisions

This slice must not add a special defer evaluator or alter the source syntax.
The existing FIR cleanup model is the implementation under test; a failing
fixture will be traced to lexical scope tracking or FIR control flow first.

## Completion record

Pending implementation and validation.
