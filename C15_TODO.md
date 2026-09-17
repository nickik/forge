# C15 compiler TODO — SIA32 floating point

## Current target boundary

Forge `f32` and `f64` are implemented and exercised through the native AArch64
compiler path. They are deliberately **not** part of SIA32 M5: Forge rejects
float-bearing FIR for SIA32 before it reaches Cranelift SIA32 ISLE lowering.

This is a target capability boundary, not a language restriction. Do not remove
the rejection merely because scalar float FIR can be produced or because
another backend accepts it.

## Prerequisites before enabling SIA32 floats

- [ ] Freeze the SIA32 floating-point ISA extension, register file, and ABI.
- [ ] Decide whether calls use float registers, integer bit-pattern registers,
      or memory-only passing; update the normative SIA ABI and Forge ABI
      decomposition together.
- [ ] Add SIA32 Cranelift `f32`/`f64` register classes and ISLE rules only once
      the architectural register contract exists.
- [ ] Implement and test constants, arithmetic, negation, comparisons, loads,
      stores, conversions, and branch conditions through production lowering.
- [ ] Define NaN, signed-zero, rounding, exception/trap, and conversion
      semantics at the Forge/SIA boundary.
- [ ] Add SIA32 object/image and Lighting execution fixtures, including ABI
      calls and float fields inside aggregates.

Until every item is complete, retain the explicit SIA32 rejection and keep
AArch64 float fixtures in the C14 native executable specification.
