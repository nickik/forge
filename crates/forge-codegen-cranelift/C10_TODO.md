# C10 — relocatable object emission

Goal: turn the C9-complete FIR -> CLIF backend into a real relocatable native-code producer without changing Forge ABI/layout semantics.

## C10a — module/object model

- [x] define deterministic object symbol identity for every FIR function
- [x] define linkage/visibility policy for module-local and externally visible functions
- [x] build an object-module plan from a prepared FIR module before byte emission
- [x] reject symbol collisions and unsupported symbol states explicitly
- [x] preserve exact C9 Forge signatures/ABI plans at symbol boundaries
- [x] add deterministic-order and symbol-policy tests on AArch64 and RV64

## C10b — object emission

- [ ] add the object-emission dependency/API needed by the Cranelift backend
- [ ] emit every prepared function into one target relocatable object
- [ ] keep the existing relocation-free `emit_machine_code` API working
- [ ] expose object bytes plus target/module metadata through a stable backend result type
- [ ] test object format, sections, symbols, and deterministic emission

## C10c — relocations

- [ ] translate direct calls into object relocations rather than rejecting them
- [ ] translate first-class `FunctionRef` addresses into object relocations
- [ ] resolve references to functions defined in the same Forge module
- [ ] retain explicit diagnostics for unresolved/unsupported relocation forms
- [ ] test scalar and C9 aggregate calls across actual symbol boundaries

## C10d — linked executable validation

- [ ] link emitted AArch64 objects into runnable test executables
- [ ] run scalar cross-function calls through the linked image
- [ ] run direct/indirect C9 aggregate parameter and return cases
- [ ] validate hidden aggregate-return storage across the machine-code boundary
- [ ] add RV64/QEMU linked execution where the CI environment supports it
- [ ] run formatting, workspace check/tests, frontend tests, conformance and Clippy

## Completion gate

C10 is complete only when a non-trivial Forge FIR module containing multiple functions can be lowered through the C9 Forge ABI, emitted as a relocatable native object, linked, and executed successfully on AArch64, with RV64 object/linked coverage retained where available.

C10 does not redefine Forge layout or ABI. Bugs discovered there are C9 regressions and should receive focused regression tests rather than new C10 policy.
