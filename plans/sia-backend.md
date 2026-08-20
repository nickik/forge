# Forge → SIA Backend Execution Plan

**Status:** initial execution plan  
**Target:** `SIA32-I` fixed-16-bit architecture, currently draft v0.4 plus the committed `ADC`/`SBB` primary opcodes on the SIA integration branch  
**Primary goal:** compile a useful subset of Forge to real SIA machine code, run it in the SIA interpreter, and verify end-to-end semantics with automated tests.

This is an execution plan, not a hidden language specification. Forge language semantics remain normative in `docs/forge-v1-spec.md`; SIA instruction/ABI semantics belong in the SIA repository.

---

## Goal

The first complete result should make the following workflow real:

```text
Forge source
  -> Forge parser
  -> HIR / typed HIR
  -> FIR typed three-address IR
  -> SIA machine IR
  -> SIA register allocation / frame layout
  -> SIA encoder
  -> linked flat SIA image
  -> SIA reference interpreter
  -> expected exit status / stdout
```

A concrete first-success command should look approximately like:

```sh
cargo run -p forgec -- \
    --target sia32-sim \
    tests/sia/programs/add.fg \
    -o target/sia/add.bin

/path/to/siaemu target/sia/add.bin
```

and the test harness must prove that the executed result is correct.

The initial target is deliberately a **whole-program simulator target**. It does not require ELF, a system linker, an assembler, an operating system, or the full Forge standard library before arithmetic code can execute.

The eventual target should additionally support a normal object/executable model:

```text
Forge source -> SIA ELF32 relocatable object -> SIA linker -> SIA ELF32 executable
```

but ELF must not block initial code-generation experiments.

---

## Normative basis

The backend must preserve the Forge v1 semantics already documented in `docs/forge-v1-spec.md`, particularly:

- sections 6-8: typed declarations, `val`/`var`, definite initialization;
- section 9: fixed-width integer types and pointer-sized integers;
- sections 22-26: strict conversions, logical/bitwise operators, checked overflow, defined division/shift behavior, left-to-right evaluation;
- section 27: `if` / loops;
- section 33: positional `fn` calls;
- section 62: foreign ABI boundary principles;
- the global requirements that bounds checks and ordinary integer-overflow checks remain enabled in production semantics.

Compiler structure must follow `docs/compiler-architecture.md` and `docs/frontend-ir.md`:

```text
AST -> HIR -> typed HIR -> FIR -> optimization -> register allocation -> target backend
```

Do **not** implement a permanent AST-direct-to-SIA shortcut. A temporary debug helper may inspect AST, but executable code generation must consume semantically checked FIR.

The target architecture is the SIA repository's fixed-16-bit `SIA32-I` design. The first backend must not add speculative packed/SIMD instructions or other recent brainstorming that is not in the current SIA specification.

---

## Current state

### Forge

The repository currently contains one Rust workspace crate, `forge-frontend`.

Already present:

- Logos lexer;
- Chumsky parser;
- source-faithful spanned AST;
- parsing for functions, typed parameters, local/global values, integer/bool/string literals, arithmetic/comparison/bitwise/logical expressions, calls, indexing, `if`, `while`, `return`, `defer`, and unsafe blocks;
- parser tests and JSON AST dumping.

Still missing before a real backend:

- assignment parsing is not yet complete;
- module/name resolution;
- HIR;
- type checking / typed HIR;
- FIR;
- CFG construction;
- optimization;
- register allocation;
- target abstraction;
- linker/image writer;
- compiler driver.

### SIA

SIA already has a dependency-free Rust interpreter that executes raw little-endian images at address zero. The interpreter currently provides:

- 16 registers with `r0 = 0`;
- 32-bit memory/address semantics;
- fixed 16-bit instruction fetch;
- the current prototype instruction map;
- scaled indexed word load/store;
- scalar load/store and update forms;
- pair/quad load/store;
- branches/calls;
- `LDPC.W`;
- arithmetic/compare/logic/shift operations;
- `ADC` and `SBB` at primaries `D` and `E`;
- simulator semihosting using `TRAP`;
- tracing and a step limit.

The SIA spec still marks many numeric encodings and several semantic details provisional. `ADC`, `SBB`, and `EXT` are now deliberate primary assignments on the SIA integration branch, but that branch should be merged or otherwise pinned before Forge CI depends on it.

---

## Invariants

1. **Forge semantics win over convenient SIA behavior.** For example, if SIA masks a shift count but Forge requires an out-of-range shift to trap, the backend must emit a check before the SIA shift.
2. **No backend-only language semantics.** The SIA backend may define ABI/layout details, but may not silently change Forge source meaning.
3. **No undefined-overflow shortcuts.** Checked Forge arithmetic must trap on overflow regardless of optimization level.
4. **No hidden dependence on a host C compiler.** The SIA path must produce executable SIA code directly.
5. **Deterministic output.** The same source and target profile should produce identical machine code absent an explicitly variable build ID.
6. **Interpreter/compiler encoding agreement must come from one source of truth.** Do not maintain two independent hand-written opcode tables indefinitely.
7. **The first executable format may be flat binary, but the compiler design must preserve sections, symbols and relocations internally so ELF can be added without rewriting code generation.
8. **The first subset may be intentionally small; rejected Forge constructs must receive explicit diagnostics rather than being miscompiled.**

---

# Architecture decisions for the first implementation

## 1. Build semantic compiler stages before native lowering

Add a new compiler crate rather than expanding the parser into a semantic compiler.

Recommended initial structure:

```text
crates/
  forge-frontend/          existing lexer/parser/AST
  forge-compiler/          HIR, semantic analysis, FIR, common optimization
  forge-codegen-sia/       SIA machine IR, ABI, RA integration, encoding/linking
  forge-driver/            `forgec` command-line driver
```

Keep `forge-compiler` internally modular rather than immediately splitting every compiler stage into a separate crate.

Suggested modules:

```text
forge-compiler/src/
  hir.rs
  resolve.rs
  types.rs
  typeck.rs
  fir.rs
  lower_fir.rs
  cfg.rs
  optimize.rs
  target.rs
  diagnostic.rs
```

`forge-codegen-sia` should depend on FIR and a target-independent target interface, not on parser AST types.

---

## 2. First supported Forge subset

The first runnable SIA backend should intentionally support only enough Forge to exercise real code generation.

### Milestone-1 language subset

- one source module;
- `fn` definitions;
- `main() -> i32`;
- parameters and return values using `i32`, `u32`, `bool`;
- local `val` bindings;
- local `var` once assignment parsing exists;
- integer literals;
- unary integer negation / bitwise not where semantically valid;
- `+`, `-`, comparisons, `&`, `|`, `^`, shifts;
- function calls with up to six scalar arguments initially;
- `return`;
- `if` / `else`;
- `while` once mutable assignment is available;
- checked arithmetic by default;
- explicit wrapping addition/subtraction once FIR has distinct checked/wrapping operations.

### Explicitly defer from the first runnable slice

- floats;
- `i64` / `u64`;
- structs/tagged unions;
- slices/arrays beyond simple later tests;
- closures;
- `Result` / `Option` lowering;
- `defer` execution;
- allocators;
- CSP;
- FFI;
- packed/SIMD target extensions.

Unsupported constructs must fail during semantic or target validation with a clear diagnostic such as:

```text
error: SIA bootstrap backend does not yet lower f64
```

rather than reaching an encoder panic.

---

# Required SIA toolchain contract

The compiler cannot safely target a prose architecture whose exact binary behavior changes independently of the interpreter. Before the backend is considered stable, the SIA repository should provide a **toolchain contract** covering the subset Forge uses.

## SIA change 1: shared Rust ISA library

Extract encoding/decoding from `siaemu` into a reusable SIA-owned crate, for example:

```text
SIA/
  crates/sia-isa/
  crates/sia-sim/
  interpreter/       optional CLI wrapper or renamed to sia-sim
```

`SIA/sia-isa` should contain:

```rust
pub enum Instruction { ... }
pub fn encode(inst: Instruction) -> Result<u16, EncodeError>;
pub fn decode(word: u16) -> Result<Instruction, DecodeError>;
```

plus:

- architectural register constants;
- immediate/range validation;
- branch displacement helpers;
- the committed primary/subfunction map;
- ABI constants once frozen;
- relocation-kind definitions once ELF/object support starts.

The SIA interpreter must decode through this same crate. Forge should then depend on a pinned SIA revision/tag rather than duplicating opcode literals.

This is the most important cross-repository integration step.

## SIA change 2: freeze code-generator-critical encodings

The compiler needs exact definitions for at least:

- `ADD`;
- destructive `SUB`;
- `ADDO` / `SUBO`;
- `ADC` / `SBB`;
- `CMPEQ` / `CMPLT` / `CMPLTU`;
- `AND` / `OR` / `XOR`;
- `SHL` / `SHR` / `SAR` and any immediate shift form used;
- `LI` / `ADDI`;
- scalar `LB/LBU/LH/LHU/LW/SB/SH/SW`;
- post-increment / pre-decrement word forms;
- `LDP/STP/LD4/ST4` modes;
- scaled indexed `LDA.W` / `STA.W`;
- `LDPC.W`;
- `BNZ`, `DBNZ`, and preferably `BZ` if retained;
- `B` / `BL`;
- `JALR` / `RET` alias behavior;
- `TRAP`, `BREAK`, `NOP`.

The optional M extension can remain separately profiled.

## SIA change 3: freeze PC-relative rules

The current interpreter uses a prototype `LDPC.W` base of:

```text
align_down(PC + 4, 4)
```

That must become architectural or be replaced deliberately before the compiler starts emitting permanent binaries.

Likewise define, exactly:

- whether branch displacements are relative to current PC or next PC;
- signed displacement width;
- scaling unit;
- behavior on target overflow;
- long-branch veneer expectations.

## SIA change 4: define the SIA32 software ABI

The ISA and ABI should remain separate documents, but Forge needs an ABI immediately.

Proposed initial ABI to validate and then freeze in SIA:

```text
r0       zero
r1-r6    argument / result registers
r7-r8    caller-saved temporaries
r9-r12   callee-saved
r13      sp
r14      lr
r15      frame pointer when needed, otherwise callee-saved general register
```

Proposed rules:

- stack grows downward;
- stack is 16-byte aligned at public call boundaries;
- scalar return value in `r1`;
- first six scalar argument words in `r1-r6`;
- additional argument words in a caller-created stack argument area;
- `r1-r8` and `r14` are call-clobbered;
- `r9-r13` are preserved; `r15` preserved when used;
- a non-leaf function saves `r14` before making another call;
- `LDP/STP/LD4/ST4` should be used for grouped callee-save traffic where profitable.

Later ABI additions must define:

- 64-bit values as register pairs;
- aggregate returns / hidden sret pointer;
- struct layout;
- `str` and slice register/stack passing;
- floating-point ABI when an FP extension exists;
- varargs if Forge ever needs a foreign varargs bridge.

### Proposed bool representation

SIA comparisons naturally produce `0x00000000` or `0xFFFFFFFF`.

A good Forge-on-SIA ABI candidate is:

```text
register bool false = 0x00000000
register bool true  = 0xFFFFFFFF
memory bool false   = 0x00
memory bool true    = 0xFF
sizeof(bool)         = 1
```

This is attractive because storing the low byte of a register mask with `SB` preserves canonical bool, and signed byte load `LB` recreates the full-register mask without extra normalization. This must be documented as a SIA Forge ABI choice, not silently embedded in codegen.

## SIA change 5: decide stack-slot addressing after measurement

This is the most likely ISA pressure point the Forge compiler will expose.

SIA's excellent update and pair/quad operations help function prologues, but arbitrary compiler spills and stack locals need repeated access to fixed frame offsets. The current base has no general `LW/SW [base + small signed displacement]` form.

The backend should initially implement `LOAD_SLOT` / `STORE_SLOT` machine pseudos using legal existing sequences, for example:

```asm
MOV  rT, fp
ADDI rT, offset
LW   rX, [rT]
```

and measure:

- generated bytes;
- dynamic instruction count;
- spill frequency;
- pressure on temporary registers.

If stack accesses are a material problem, revisit SIA before v1.0. Candidate fixes include reusing the four currently unused scalar-memory modes for common frame offsets (`+4`, `+8`, etc.) or a more systematic compact stack addressing rule. Do not change the ISA preemptively; let actual Forge code quantify the cost.

## SIA change 6: freeze divide/overflow semantics for `SIA-M`

Forge requires defined traps for:

- divide by zero;
- signed division overflow;
- out-of-range checked shifts.

If SIA hardware semantics do not exactly match Forge, the backend must emit explicit checks. The architecture should still define SIA behavior precisely so the optimizer knows which checks are redundant.

---

# FIR design required for SIA

FIR should remain target-independent and typed.

A minimal initial operation set:

```text
const.i32
const.u32
copy
add.checked
add.wrap
sub.checked
sub.wrap
and
or
xor
shl.checked
shr.checked
cmp.eq
cmp.lt.s
cmp.lt.u
br
br_if
call
ret
trap
```

Later:

```text
mul.checked / mul.wrap
div / rem
load / store
stack.alloc
index.checked
```

Every FIR value has an exact type. Forge bool remains an abstract `bool` in FIR; the SIA mask representation is introduced only by target lowering.

FIR should use explicit basic blocks and terminators:

```text
bb0:
  %3:i32 = add.checked %1, %2
  %4:bool = cmp.lt.s %3, 10
  br_if %4, bb1, bb2
```

The first optimizer should perform only the already-planned simple passes: constant folding, algebraic simplification, copy propagation, DCE and block simplification.

---

# SIA lowering model

## Machine IR

Do not encode directly while walking FIR. Introduce a small SIA Machine IR with virtual registers and labels.

Example conceptual forms:

```text
SiaMI::Add3      { dst, lhs, rhs }
SiaMI::Sub2      { dst, rhs }
SiaMI::CmpLtS2   { dst, rhs }
SiaMI::Mov       { dst, src }
SiaMI::LoadImm   { dst, value }          // pseudo
SiaMI::LoadSlot  { dst, slot }           // pseudo
SiaMI::StoreSlot { src, slot }           // pseudo
SiaMI::Br        { label }
SiaMI::BrNz      { cond, label }
SiaMI::Call      { symbol }               // pseudo/relocation
SiaMI::Ret
```

Pseudo instructions survive until frame layout / branch relaxation as necessary.

## Destructive-operation handling

FIR is three-address; much of SIA is two-address.

For:

```text
%z = and %a, %b
```

prefer coalescing `%z` with `%a`:

```asm
AND z,b
```

If liveness prevents that, insert:

```asm
MOV z,a
AND z,b
```

Do not distort FIR into destructive form merely because SIA uses it.

Machine IR should represent tied operand preferences so the allocator can reduce moves.

## Register allocation

Implement the repository's planned linear scan allocator.

Initial allocatable pool should exclude:

- `r0` always;
- `r13` stack pointer;
- `r14` link register during ordinary value allocation;
- `r15` when a frame pointer is required.

Allow `r15` as a general callee-saved register in frameless functions later.

Allocator features needed early:

- live intervals;
- caller/callee-save awareness;
- preferred/tied registers for destructive instructions;
- rematerialization of small constants;
- spill slots;
- simple live-range splitting later.

## Frame layout

Each function should calculate:

```text
outgoing stack args
spill slots
local stack storage
saved callee registers
saved link register
alignment padding
```

Leaf functions with no spills should be frameless whenever possible.

Use `STP/ST4` and `LDP/LD4` for grouped saves/restores where register sets are consecutive.

---

# Integer lowering details

## Constants

Use `LI` for representable signed small immediates.

Use a literal-pool pseudo for larger values:

```text
LoadImm r4, 0x12345678
    -> LDPC.W r4, .Lconst
```

The linker/literal placer resolves the final displacement.

## Checked signed addition/subtraction

Forge `i32` checked operations should use:

```text
ADDO
SUBO
```

when their SIA semantics exactly match Forge.

## Checked unsigned addition/subtraction

Use `ADC` / `SBB` with an explicit zero carry/borrow temporary and branch/trap on the resulting mask.

Conceptually:

```asm
LI   rc, 0
MOV  rd, lhs
ADC  rd, rhs, rc
BNZ  rc, overflow
```

The optimizer/register allocator should coalesce the destination where possible.

## Wrapping addition/subtraction

Use normal `ADD` / destructive `SUB` with no overflow branch.

## Comparisons

Map directly where possible:

```text
==   -> CMPEQ
< s  -> CMPLT
< u  -> CMPLTU
```

Synthesize `!=`, `<=`, `>`, `>=` through operand reversal and mask inversion rather than requiring new SIA instructions.

## Bool control flow

SIA's mask booleans map cleanly to:

```asm
BNZ rCond,label
```

A `BZ` form is useful but not required for correctness; invert CFG edges if necessary.

## Shifts

SIA register shifts mask the shift count to the low five bits. Forge checked shifts require a trap if the count is outside the valid range.

Therefore ordinary Forge shifts require:

```text
check count < 32
shift
```

unless compile-time range analysis proves the check unnecessary.

## Multiply/divide

Define target feature profiles:

```text
sia32-i       mandatory base only
sia32-im      base + SIA-M
```

For `sia32-i`, multiplication/division lower to runtime helpers until software expansion is implemented.

For `sia32-im`, use the SIA extension but retain any Forge checks not guaranteed by hardware semantics.

Do not make the whole compiler assume `M` merely because the simulator currently implements it.

---

# Calls and entry point

## Bootstrap entry

For flat simulator images, the compiler/linker should synthesize `_start` at address zero.

Conceptually:

```asm
_start:
    BL   forge_main
    ; main scalar result is already in r1
    TRAP 0
```

The simulator initializes `sp`, so no loader ABI is required for the first phase.

The source function remains ordinary Forge:

```forge
fn main() -> i32 {
    return 42;
}
```

The compiler should diagnose an unsupported bootstrap `main` signature rather than inventing implicit arguments.

## Function calls

Initial call support:

- up to six scalar word arguments in `r1-r6`;
- scalar result in `r1`;
- direct `BL` when in range;
- a relocation/veneer pseudo for calls whose final range is unknown;
- save `lr` in non-leaf functions.

Later add stack arguments and multiword returns after frame-addressing experiments.

---

# Output and `println`

## Architectural rule

`println` must **not** become a SIA instruction.

Normally, a compiled language implements output as:

```text
println formatting
  -> standard-library write routine
  -> runtime / OS write service
  -> device driver / terminal
```

For a simulator without an OS, semihosting substitutes for the runtime/OS boundary.

The current SIA interpreter already provides:

```text
TRAP 0  exit(r1)
TRAP 1  write(fd=r1, address=r2, length=r3)
TRAP 2  putchar(low_byte(r1))
```

These are simulator conventions, not architectural SIA I/O.

## Bring-up strategy

### Stage A: no formatting required

The earliest arithmetic tests should use the process exit value:

```forge
fn main() -> i32 {
    return 42;
}
```

The test script expects interpreter exit status `42`.

This is enough to prove parse -> semantic lowering -> FIR -> SIA -> execution before a standard library exists.

### Stage B: tiny target runtime

Add target runtime symbols such as:

```text
__forge_exit(i32)
__forge_write(fd: u32, ptr: *u8, len: usize)
__forge_putchar(byte: u8)
```

On `sia32-sim`, these wrappers use the interpreter's semihosting traps.

Forge source should call ordinary library/runtime functions; only the simulator runtime knows that `TRAP 1` exists.

### Stage C: `println`

Implement formatting in the Forge standard library/runtime, not in codegen.

First useful functions:

```text
print(str)
println(str)
print_i32(i32)
print_u32(u32)
```

Integer formatting can itself become a useful Forge-on-SIA compiler test once loops, division/remainder or a software helper are operational.

---

# Flat image linker: first executable format

ELF is not required for the first end-to-end milestone.

The compiler should nevertheless build a sectioned in-memory object model:

```text
.text
.rodata
.data
.bss
symbols
relocations
```

For `sia32-sim`, an internal mini-linker resolves the whole program and emits a raw image.

Suggested initial layout:

```text
0x00000000  _start
            .text
            per-function literal pools / islands
            .rodata
            .data
            [end of file]
            .bss in zeroed simulator RAM
```

`_start` must terminate through `TRAP 0`, so execution never depends on falling through into data.

## Initial relocation/fixup kinds

Even for raw images, model fixups explicitly:

```text
Abs32           32-bit absolute data/address word
Branch11        B/BL relative branch
CondBranch7     BNZ/DBNZ relative branch
Ldpc8Word       LDPC.W word-scaled PC-relative literal
Call            abstract call, relaxed to BL or veneer
```

Names are provisional; SIA should later own the formal relocation names/numbers.

## Literal pools

Large constants and addresses use `LDPC.W`, so code generation must support literal placement.

First implementation may place a pool after each small function and reject out-of-range cases with a compiler-internal diagnostic.

Then implement literal islands/relaxation so no valid program depends on a function staying below one pool range.

## Long branches/calls

Keep `Call` and long-jump pseudos until final layout.

If direct `BL` is out of range, generate a veneer using a PC-relative loaded target address plus `JALR`.

---

# ELF32 phase

After the flat-image path is stable, add ELF as the normal SIA object/executable format.

Recommended properties:

```text
ELFCLASS32
little-endian
32-bit addresses
ET_REL for object files
ET_EXEC for linked simulator/OS executables initially
RELA relocations preferred for explicit addends
```

Required sections:

```text
.text
.rodata
.data
.bss
.symtab
.strtab
.rela.text / other relocation sections
```

The SIA repository should own:

- target ABI document;
- ELF machine identifier policy;
- relocation definitions;
- ELF processor flags if any;
- canonical relocation calculation formulas.

Until SIA receives an official ELF `e_machine` assignment, use a clearly documented project-local value accepted only by our own tools. Do not pretend an arbitrary value is globally standardized.

The first ELF implementation can either:

1. use the Rust `object` crate if it materially lowers format risk; or
2. implement a minimal auditable ELF32 writer after the required subset is frozen.

The SIA interpreter should eventually gain an ELF loader that maps `PT_LOAD` segments and starts at `e_entry`. Raw binaries can remain supported for tiny tests.

---

# Milestones

## Milestone 0 — Pin the SIA execution contract

**Outcome:** Forge development has one exact SIA revision to target.

Tasks:

1. Merge or pin the SIA branch containing permanent `ADC=D`, `SBB=E`, `EXT=F`.
2. Record the exact SIA commit in Forge integration configuration.
3. Extract/shared `sia-isa` encoding library or, if that is delayed, create a temporary Forge target table explicitly marked for later replacement.
4. Freeze the branch/LDPC semantics used by tests before generating golden binaries.

Tests:

- SIA encode/decode round trip for every instruction Forge will initially emit;
- interpreter executes words emitted by `sia-isa`.

## Milestone 1 — Minimal semantic compiler

**Outcome:** parsed Forge functions become typed HIR.

Tasks:

1. Add assignment parsing needed for useful loops.
2. Add symbol tables and local/function name resolution.
3. Add exact primitive types for `i32`, `u32`, `bool`.
4. Type integer literals contextually.
5. Type arithmetic/comparison/calls/return/if/while.
6. Enforce no implicit signed/unsigned mixing.
7. Enforce function signatures and definite initialization for supported forms.

Tests:

- positive typed examples;
- wrong return type;
- signed/unsigned mixing diagnostic;
- unresolved name;
- wrong argument count/type;
- uninitialized local use.

## Milestone 2 — FIR

**Outcome:** typed HIR lowers to deterministic FIR CFGs.

Tasks:

1. Define values, blocks, operations and terminators.
2. Lower expressions left-to-right.
3. Make checked/wrapping arithmetic distinct.
4. Lower conditionals and loops to blocks.
5. Lower calls and returns.
6. Add textual FIR dump for tests/debugging.

Tests:

- FIR golden tests;
- evaluation-order tests;
- CFG shape tests;
- checked-vs-wrapping distinction.

## Milestone 3 — Straight-line SIA codegen without spills

**Outcome:** pure arithmetic Forge programs execute in `siaemu`.

Tasks:

1. Add SIA target data layout (`usize=32`, pointer=32, LE).
2. Lower constants, add/sub, logic, comparisons and return.
3. Use a trivial temporary allocator for functions proven to fit available registers, or implement linear scan directly.
4. Emit `_start`.
5. Emit raw linked binary.
6. Execute it with `siaemu` from an integration test/script.

Required first programs:

```text
return_42.fg
add_i32.fg
sub_i32.fg
nested_expr.fg
compare_if.fg
bitwise.fg
large_literal.fg
```

## Milestone 4 — Calls, real register allocation and stack frames

**Outcome:** multiple Forge functions, nesting and register pressure work.

Tasks:

1. Implement SIA calling convention.
2. Implement linear-scan register allocation.
3. Implement tied/destructive operand preferences.
4. Implement spill slots.
5. Implement frame layout.
6. Save/restore `lr` for non-leaf functions.
7. Use pair/quad save/restore where profitable.
8. Implement call relocations and direct-call relaxation.

Required tests:

```text
call_one.fg
nested_calls.fg
six_args.fg
callee_saved.fg
spill_pressure.fg
recursive_small.fg
```

The spill test is a deliberate SIA architecture benchmark: record code size and instruction count for frame-slot traffic.

## Milestone 5 — Loops, checked arithmetic and target semantics

**Outcome:** math workloads exercise control flow and Forge safety semantics.

Tasks:

1. Complete assignment lowering.
2. Lower `while`.
3. Add checked signed add/sub.
4. Add checked unsigned add/sub via `ADC/SBB`.
5. Add checked shift-count guards.
6. Add wrapping variants.
7. Add overflow/fault test harness support.
8. Add optional DBNZ peephole after correctness is established.

Programs:

```text
sum_1_to_n.fg
factorial_small.fg
fibonacci_iter.fg
gcd.fg
unsigned_carry.fg
signed_overflow.fg
shift_range.fg
```

## Milestone 6 — Simulator runtime and output

**Outcome:** Forge programs can print deterministic text in the SIA simulator.

Tasks:

1. Add simulator runtime write/exit wrappers.
2. Add static string placement and address/length passing.
3. Implement `print` / `println` for strings.
4. Add integer-to-decimal formatting or a temporary runtime helper.
5. Capture stdout in integration tests.

Programs:

```text
hello_sia.fg
print_sum.fg
print_signed.fg
```

## Milestone 7 — Memory and arrays

**Outcome:** simple array programs use SIA's addressing strengths.

Tasks:

1. Define target layout for arrays and basic stack aggregates.
2. Lower byte/halfword/word loads/stores.
3. Lower checked indexing.
4. Select `LDA.W/STA.W` for `u32` array indexing.
5. Use post-increment forms in simple pointer-walk peepholes if profitable.
6. Measure whether fixed stack offsets need ISA improvement.

Programs:

```text
array_sum.fg
array_copy.fg
array_bounds_fail.fg
```

## Milestone 8 — M extension and/or software multiply/divide

**Outcome:** nontrivial integer math works on both base and M profiles.

Tasks:

1. Define `sia32-i` and `sia32-im` target features.
2. Lower multiply/divide to hardware for `+M`.
3. Provide base-runtime helpers for `-M`.
4. Preserve Forge overflow/divide semantics.
5. Differentially test both profiles in the interpreter.

## Milestone 9 — ELF32 and relocatable compilation

**Outcome:** Forge can emit normal SIA objects and executables.

Tasks:

1. Freeze SIA ELF ABI/relocations.
2. Emit ET_REL objects.
3. Implement or integrate SIA linker support.
4. Emit ET_EXEC.
5. Add ELF loading to `siaemu` if not already present.
6. Keep raw image mode as a bring-up/debug option.

## Milestone 10 — Broaden Forge type coverage

Add in measured order:

- `i8/u8/i16/u16` with canonical register rules;
- `usize/isize`;
- `i64/u64` using register pairs;
- `str` and slices;
- structs;
- `Option` / `Result`;
- `defer` cleanup edges;
- allocators;
- soft float or a future SIA FP extension.

---

# Test strategy

## Unit tests

### Frontend/sema

- name resolution;
- literal typing;
- arithmetic legality;
- call checking;
- diagnostics.

### FIR

Golden text snapshots for small functions.

### SIA instruction selection

For each FIR operation, assert selected machine IR and important peepholes.

### Encoder

Prefer tests in the SIA-owned `sia-isa` crate, then use that crate from Forge.

## End-to-end compile/run tests

Create:

```text
tests/sia/programs/
tests/sia/expected/
scripts/test-sia.sh
```

The script should:

1. build Forge;
2. compile each `.fg` to SIA;
3. run the image in the pinned SIA interpreter;
4. compare exit status and/or stdout;
5. fail on unexpected simulator faults;
6. support expected-fault tests for checked overflow/bounds errors.

The script should accept a sibling checkout path:

```sh
SIA_DIR=../SIA ./scripts/test-sia.sh
```

CI should check out the exact pinned SIA revision into a known path rather than depending on whatever happens to be installed on the runner.

Longer term, if `sia-sim` becomes a reusable Rust library, Forge integration tests should call it directly in-process and retain the CLI script as an independent cross-check.

## Golden binaries

Use golden machine-code blobs sparingly. They are useful only after the relevant SIA encoding is intentionally frozen. Before then, prefer semantic compile-and-run tests and decoded instruction snapshots.

## Differential testing

Once a portable C backend exists, compile the same integer test corpus to:

```text
host C
SIA
```

and compare outputs. Until then, use explicitly known results and FIR-level tests.

---

# Initial example corpus

Start with programs whose expected results are obvious and small enough to use exit codes:

```forge
module tests.return_42;

fn main() -> i32 {
    return 42;
}
```

```forge
module tests.add;

fn add(a: i32, b: i32) -> i32 {
    return a + b;
}

fn main() -> i32 {
    return add(20, 22);
}
```

```forge
module tests.branch;

fn max(a: i32, b: i32) -> i32 {
    if (a < b) {
        return b;
    } else {
        return a;
    }
}

fn main() -> i32 {
    return max(17, 42);
}
```

After assignments work:

```forge
module tests.sum;

fn main() -> i32 {
    var i: i32 = 10;
    var sum: i32 = 0;
    while (i > 0) {
        sum = sum + i;
        i = i - 1;
    }
    return sum;
}
```

Later output test:

```forge
module tests.print_sum;

import std.io;

fn main() -> i32 {
    val answer: i32 = 20 + 22;
    io.println("{}", answer);
    return 0;
}
```

The final form of standard-library formatting may differ, but output should remain a library/runtime concern rather than an ISA feature.

---

# Risks / decisions

## Risk: SIA encoding is still provisional

**Mitigation:** pin one revision and move encode/decode into a shared SIA crate before many golden tests exist.

## Risk: stack spills are expensive without base+offset memory addressing

**Mitigation:** deliberately force spills in Milestone 4 and measure before changing SIA. This is a primary architecture experiment, not an incidental compiler detail.

## Risk: only 16 architectural registers plus destructive ALU operations increase move/spill pressure

**Mitigation:** tied-register preferences, copy coalescing, constant rematerialization, frameless leaf functions, and target peepholes. Measure move count in benchmark reports.

## Risk: literal-pool range is short

**Mitigation:** keep literal references as pseudos until layout; implement per-function pools then literal islands.

## Risk: branch/call reach is short

**Mitigation:** explicit relocations and veneers; never expose branch-range limits as a Forge language restriction.

## Risk: bootstrap semihosting leaks into the language

**Mitigation:** isolate traps in `sia32-sim` runtime wrappers. `println` stays a library function.

## Risk: ELF work becomes a distraction

**Mitigation:** prove the compiler with a raw whole-program image first. Preserve sections/symbols/relocations internally so ELF is an output-format step, not a backend rewrite.

## Risk: Forge checked semantics require instructions SIA does not have directly

**Mitigation:** lower to short instruction sequences or runtime helpers first. Only propose ISA additions when generated-code measurements show a recurring material cost.

---

# SIA changes to evaluate from real Forge output

Do not make these changes merely to make the compiler writer's life easier. Instrument the backend and collect data first.

1. **Small stack/frame displacement loads/stores.** Highest-priority measurement target.
2. **`BZ`.** Useful if branch inversion causes noticeable block-layout problems; not required for correctness.
3. **PC-relative literal range/base rule.** Validate against compiled functions and literal density.
4. **Direct call range.** Validate veneer frequency.
5. **`SH2ADD`.** Measure address-generation demand outside `LDA.W/STA.W` before considering it.
6. **Narrow integer helpers.** Measure before adding target instructions; normal loads/shifts/masks may be adequate.
7. **Software-FP helpers.** Explicitly outside the first backend milestone.

No packed/SIMD work belongs in this bring-up plan.

---

# Instrumentation to add to the backend

Every compile in `--stats` mode should be able to report at least:

```text
FIR instruction count
SIA instruction count
code bytes
literal-pool bytes
number of inserted MOVs due to destructive constraints
number of spills/reloads
stack-frame size
number of direct calls
number of call veneers
number of long branch veneers
number of LDPC literal references
number of runtime helper calls
```

These measurements should guide SIA changes. In particular, the decision to add stack-relative addressing should be based on spill/local-access numbers from real Forge programs.

---

# Definition of done for the first backend phase

The first SIA backend is considered genuinely working when all of the following are true:

1. Forge source is parsed by the existing frontend.
2. Names and primitive types are semantically checked.
3. Code passes through typed FIR.
4. SIA machine code is generated directly from FIR.
5. Functions obey a documented SIA calling convention.
6. The compiler emits a complete raw simulator image with `_start`.
7. At least arithmetic, comparisons, branches, calls and returns work.
8. Register pressure can force a spill and still execute correctly.
9. Checked signed overflow produces the expected trap/fault behavior.
10. At least one unsigned carry/borrow test exercises the new SIA `ADC`/`SBB` instructions.
11. A script/CI test compiles Forge programs, runs them in `siaemu`, and validates exit results.
12. At least one program produces stdout through the simulator runtime rather than through a compiler special case.
13. Backend statistics are available to inform SIA architecture changes.

ELF, full standard-library formatting, floats, 64-bit types and the full Forge v1 language are **not** prerequisites for this first definition of done.

---

# Completion record

Not yet implemented.

When milestones are completed, update this section with:

- commits/revisions of Forge and SIA used together;
- SIA ABI/spec changes made as a result of compiler measurements;
- files/crates added;
- tests and example programs added;
- exact commands used for end-to-end validation;
- measured code-size/spill/literal/veneer statistics;
- remaining unsupported Forge features.
