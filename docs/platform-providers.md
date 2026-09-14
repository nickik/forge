# Platform provider selection

Forge build invocations accept an optional platform selector:

```text
forge build --platform host
forge build --platform lighting-sim
forge build --platform lighting
```

The build driver forwards the selection to the compiler/interpreter as:

```text
--platform NAME
```

This is a build/toolchain selection, not a Forge source-language feature. Packages should use a stable provider/module contract rather than scattering platform conditionals through ordinary source.

The immediate consumer is Cosmic OS. Its kernel logic is written in Forge above narrow machine/provider interfaces. During bootstrap, CForge accepts `--platform` and can provide host-backed services. Later `lighting-sim` selects services supplied by LightingSimulation, and `lighting` selects the real machine backend, without changing Cosmic policy/object code.

The current flag only transports platform identity to the driver. General manifest-declared provider resolution and target triples remain future build-system work.
