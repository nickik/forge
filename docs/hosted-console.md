# Hosted console library

`std.console` is the first intentionally small hosted Forge library. It provides an ergonomic application-facing console API while keeping the platform implementation behind a target-selected provider boundary.

## Public API

```forge
import std.console;

console.write("hello\n");
```

The initial operation is:

```text
write(str) -> void
```

It is intentionally smaller than a general formatting or terminal package.

## Two provider models

Forge deliberately uses two related but different provider mechanisms.

### 1. Hosted platform providers

Hosted `std` normally has exactly one implementation of an operating-system service in a final artifact. The implementation is therefore selected by the build/link environment rather than passed through every application call.

```text
application
    |
    v
std.console.write
    |
    v
Forge hosted platform ABI
    |
    +-- Unix/POSIX provider
    +-- Cosmic userspace provider
    +-- CForge host provider
    +-- another hosted provider
```

The bootstrap console hook is conceptually:

```text
__forge_console_write(text: str) -> void
```

`std.console` owns the portable API. The selected platform runtime owns the implementation of that private hook. The hook is an ABI/library contract, not a new source-language feature and not a user-visible interface or trait.

A final hosted artifact must bind exactly one compatible implementation for every platform hook used by `std`. Missing or incompatible hooks are link/build errors.

This model is appropriate for process-wide services such as:

- process console/stdout/stderr;
- filesystem syscalls;
- clocks;
- entropy;
- OS threads;
- process/environment services;
- hosted virtual-memory acquisition.

### 2. Explicit capability providers

When several providers or instances may coexist inside one program, the provider remains an ordinary Forge value using the `context + Ops` pattern already used by `Arena` and `core.io.Writer`:

```text
Capability {
    context
    ops -> function table
}
```

This is appropriate for arenas, allocators, writers, devices, in-memory test sinks, per-object resources and kernel facilities.

The distinction is intentional:

```text
one implementation per final hosted artifact -> platform ABI/provider binding
many instances/providers at runtime         -> explicit context + Ops capability
```

Forge therefore does not need traits or interfaces to express either case.

## Platform implementations are replaceable

The portable `std` source must not contain Unix, libc, JVM or Cosmic-specific behavior. A platform package/runtime supplies the private hosted ABI implementation for its target.

Conceptually:

```text
std portable code
      |
      v
private std platform ABI
      |
      +-- forge-platform-unix
      +-- forge-platform-cosmic
      +-- cforge-host
```

The exact packaging/binding syntax belongs to the build/library model, not the Forge language grammar. Explicit library mappings must permit alternate implementations for tests and new targets.

## Relationship to freestanding output

`std.console` is hosted convenience. It is unavailable under `--no-std`.

Kernel, firmware, bootloader and other freestanding code should use `core.io.Writer` or another explicitly supplied capability. This prevents the kernel from depending on a process-console model while allowing the same higher-level formatting algorithms to target serial ports, framebuffers, in-memory logs or hosted standard output.

## Bootstrap status

CForge currently recognizes `import std.console` and dispatches `console.write` through its replaceable host provider. This lets runnable Forge applications exercise the API before the full multi-compilation-unit library linker is complete.

The production Forge compiler should ultimately resolve `std.console` through the ordinary library/module graph and bind its private platform hook from the selected target runtime. At that point the builtin recognition in CForge becomes a bootstrap compatibility implementation rather than the normative loading mechanism.
