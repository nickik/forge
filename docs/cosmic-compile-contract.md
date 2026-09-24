# Forge-to-Cosmic compile contract

This document freezes the current bootstrap boundary between Forge and Cosmic.
It describes image production only. Successful compilation does not claim that
the image has executed on LightingSimulation.

## Source and module inputs

- One root Forge source file supplies the kernel or userspace entry function.
- Each dependency is mapped explicitly as `--library NAME=PATH`.
- `NAME` is a semantic module name, not a filesystem import. The source at
  `PATH` must declare that module.
- Forge constructs and validates the complete module graph before SIA32
  lowering. Visibility failures, missing symbols and dependency cycles are
  compiler errors; callers must not concatenate source files.
- Freestanding compilation does not inject hosted `std` or a hosted runtime.

## SIA32 userspace image

The System Task/userspace command is:

```sh
forge-lighting-firmware system-task.fg \
  --library cosmic.abi=path/to/abi.fg \
  --entry system_task_entry \
  --user-image \
  --text-base 0x00200000 \
  -o system-task.bin
```

The contract is:

- `--entry` names a zero-argument Forge function returning `i32`.
- `--text-base` is mandatory and is the virtual address at which the consumer
  will map and execute the image.
- The output is deterministic, headerless, linked SIA32 text. Its entry is the
  first byte and is linked at the requested address.
- Calls between the root and explicit libraries are resolved through SIAO32
  relocations before the flat image is written.
- `--user-image` cannot embed a boot payload. Kernel loading, address-space
  construction, the user stack and privilege transition belong to Cosmic and
  LightingSimulation.

The current M28.5 proof source additionally uses the architectural syscall
exchange in `r1`, `TRAP 0x40`/`0x41`, and a userspace proof-word store. Image
emission checks those compiler boundaries. Only the real Lighting boot gate can
prove `SRET`, register preservation, the dedicated user stack and the observed
proof word.

## Kernel and reset-ROM inputs

`forge-lighting-firmware` also supports the existing raw-image and reset-ROM
bring-up modes. Those modes share the same semantic module linker and
production Cranelift SIA32 backend, but they are separate artifacts from a
userspace image. A userspace image must not be substituted for a reset ROM or
silently bundled into one.

## Acceptance boundary

Forge CI owns deterministic semantic linking and image emission. The owning
LightingSimulation gate must load the emitted bytes at the same `--text-base`,
enter them through Cosmic's real kernel/user transition, and assert the
architectural result. Native AArch64/RISC-V execution and SIA32 image creation
remain supporting evidence, not a replacement for that simulator proof.
