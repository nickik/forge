# Goal
Link existing Cosmic Task/AddressSpace Forge modules into the freestanding SIA32 boot image with explicit native storage and machine providers.

# Normative basis
Forge v1 library specification sections 5–8: source modules, explicit library bindings and replaceable providers. Existing unsafe pointer and SIA intrinsic semantics remain unchanged.

# Current state
The firmware driver compiles one source only. The hosted compiler already has an AST module linker. Cosmic's M28 image uses a raw three-word context instead of its existing object bootstrap.

# Invariants
Reuse the semantic module linker; reject missing providers and unsupported native functions. No hosted runtime, C code or simulator state injection. Kernel policy remains existing Forge modules. Preserve concurrent working-tree changes.

# Milestones
1. Expose AST linking and explicit firmware library bindings; eliminate unreachable functions before native emission.
2. Supply bounded native bootstrap storage and physical table-memory providers; run real object construction.
3. Integrate the ROM boot descriptor, saved context, syscall preservation and user fault proof.

# Tests
Firmware module positive/negative tests, provider bounds/exhaustion tests, existing M28 architectural ABI tests and full reset/ROM integration.

# Risks / decisions
The bootstrap raw-storage contract is legacy and bounded; it is not the final allocator API. ROM capacity and frontend support for the semantic dependency graph must be measured. Hardware PTE realization must not be confused with logical mappings.

# Completion record
Completed locally: explicit firmware library bindings reuse the AST linker, reachable native code rejects raw hosted placeholders, and pointer aliases resolve in the type namespace. Embedded ROM text uses its actual relocated entry address. Cosmic now runs its existing object graph through bounded native providers and the real ROM handoff.

The SIA backend is pinned to committed revision d99d450188abd5cbaa76517300abd6d652caa1ee, which fixes boolean comparison lowering and preservation of caller frames across calls. Unsigned checked arithmetic retains overflow traps using supported instructions.

Validation: module linker tests (3), firmware module integration tests (3), scalar arithmetic tests (11), existing Lighting boot/user ABI regressions, and full M28 reset/ROM/object/syscall/protection-fault gate passed. No remote CI or publication is claimed.

# M29 extension
Raw supervisor entries may accept up to two u32 words and return i32/u32, using the existing scalar ABI. Reset-ROM and user-image entries retain their zero-argument i32 contract. This lets one native kernel image handle boot and architectural trap calls, avoiding duplicate object code in the bounded ROM. Positive scalar-entry and negative non-word-argument tests pass locally (five firmware integration tests total). The M29 native gate exercises actual register-frame calls through the same scalar ABI.
