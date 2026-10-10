# M30 native arithmetic lowering plan

## Goal and basis
Compile Cosmic's existing QDX semantic modules to SIA32 while preserving Forge
v1 checked integer semantics (spec integer arithmetic/overflow sections).

## Current state
SIA cannot select Cranelift sadd_overflow/ssub_overflow/smul_overflow operations.
Unsigned subtraction and multiplication already use baseline equivalent checks.

## Invariants and milestones
Use baseline integer arithmetic and comparisons to compute signed overflow.
Keep the overflow trap and wrapping behavior unchanged across targets. No syntax
or ABI changes. Validate signed extrema, zero and MIN times -1, existing scalar
CLIF regressions and native Cosmic QDX vectors.

## Tests, risks and completion
CLIF tests must assert semantics rather than require specialized op spellings.
Signed multiplication uses unsigned magnitudes and a sign-dependent limit to
avoid signed division overflow. Implementation pending.
