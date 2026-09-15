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

- [x] add the object-emission API needed by the Cranelift backend
- [x] emit every prepared function into one target relocatable object
- [x] keep the existing relocation-free `emit_machine_code` API working
- [x] expose object bytes plus target/module metadata through a stable backend result type
- [x] test object format, sections, symbols, and deterministic emission

## C10c — relocations

- [x] translate direct calls into object relocations rather than rejecting them
- [x] translate first-class `FunctionRef` addresses into object relocations
- [x] resolve references to functions defined in the same Forge module
- [x] retain explicit diagnostics for unresolved/unsupported relocation forms
- [x] test scalar and C9 aggregate calls across actual symbol boundaries

## C10d — linked executable validation

- [x] link emitted AArch64 objects into runnable test executables
- [x] run scalar cross-function calls through the linked image
- [x] run direct/indirect C9 aggregate parameter and return cases
- [x] validate hidden aggregate-return storage across the machine-code boundary
- [x] add RV64/QEMU linked execution where the CI environment supports it
- [x] run formatting, workspace check/tests, frontend tests, conformance and Clippy

## Completion record

Final CI run `34990436853` passed formatting, workspace check, build-system tests, generated collections snapshot, frontend tests, workspace tests, conformance, and Clippy. Linked execution covers native AArch64 and RV64/QEMU, including direct and indirect calls plus C9 aggregate ABI paths.

C10 does not redefine Forge layout or ABI. Bugs discovered there are C9 regressions and should receive focused regression tests rather than new C10 policy.
