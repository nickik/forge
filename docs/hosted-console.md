# Hosted console library

`std.console` is the first intentionally small hosted Forge library. It provides an ergonomic application-facing console API while keeping the platform implementation behind a provider boundary.

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

## Provider boundary

Application source depends on `std.console`, not directly on Unix, libc, JVM facilities, or Cosmic system calls.

```text
application
    |
    v
std.console.write
    |
    v
hosted console provider
    |
    +-- Unix/POSIX runtime
    +-- CForge host runtime
    +-- Cosmic userspace runtime
    +-- another hosted target
```

The bootstrap provider hook is conceptually:

```text
__forge_console_write(text: str) -> void
```

This hook is a runtime/platform implementation detail rather than a new source-language facility.

## Relationship to freestanding output

`std.console` is hosted convenience. It is unavailable under `--no-std`.

Kernel, firmware, bootloader and other freestanding code should use `core.io.Writer` or an environment-specific writer capability. This prevents the kernel from depending on a process-console model while allowing the same higher-level formatting algorithms to target serial ports, framebuffers, in-memory logs or hosted standard output.

## Bootstrap status

CForge currently recognizes `import std.console` and dispatches `console.write` through its replaceable host provider. This lets runnable Forge applications exercise the API before the full multi-compilation-unit library linker is complete.

The production Forge compiler should ultimately resolve `std.console` through the ordinary library/module graph. At that point the builtin recognition in CForge becomes a bootstrap compatibility implementation rather than the normative loading mechanism.
