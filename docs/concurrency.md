# Forge v1 Concurrency Model

## Philosophy

Forge v1 does not require green threads or a language scheduler. Its required concurrency foundation is deliberately small: operating-system threads/processes where available and explicit synchronization primitives. Higher-level communication and scheduling abstractions are library/version evolution rather than requirements of the v1 language or runtime ABI.

## Threads

`std.thread` exposes OS-backed thread creation, join, yield/sleep and thread-local execution context. Targets without threads reject thread-dependent programs at compile/link configuration time rather than pretending to support them.

Each thread receives a derived `context`, including its own scratch arena unless explicitly overridden.

## Atomics

Provide concrete atomic scalar/pointer types in v1, e.g. `Atomic_u32`, with explicit relaxed/acquire/release/acq_rel/seq_cst ordering where supported.

A convenience `update` operation may apply a non-escaping function repeatedly using compare-exchange:

```forge
counter.update(
    (x: u32) -> u32 { return x + 1; }
)?;
```

The update function may execute more than once and therefore must not perform externally visible side effects.

## Future work: channels and `select`

Typed channels, `send`/`recv`, and multi-wait `select` are **not part of Forge v1**. They are explicitly deferred to future language/library work.

Earlier design sketches showed CSP-style channels and language-level `select` syntax. Those sketches are non-normative and must not be treated as C14/C14e implementation requirements. Forge v1 therefore defines no:

- `Channel[T]` representation or standard-library channel API;
- channel creation, send, receive, close, buffering, or cross-process semantics;
- `select`, `recv`, or `timeout` source syntax;
- channel/select FIR or native runtime ABI requirement;
- fairness, wakeup, ordering, cancellation, or timeout rules for multi-wait operations.

A future Forge version may add channels and/or a selector facility after their semantics, ownership model, runtime boundary, freestanding implications, and interaction with Cosmic have been designed deliberately. Such a design must not be inferred from currently retained parser/compiler experiments.

Compiler code that already recognizes or models experimental `select` forms may remain temporarily as dormant implementation scaffolding, but it is outside the Forge v1 compatibility surface and does not need native lowering for C14 completion.

## Agents: move work to data

An Agent owns mutable state and serializes actions applied to that state. This is inspired by the idea of sending a function/action to the data rather than sharing arbitrary mutation.

Within one address space, an action can be represented by a function pointer plus copied argument data:

```forge
fn deposit(account: &mut Account, amount: Money) {
    account.balance += amount;
}

accounts.send(deposit, amount)?;
```

The agent worker executes actions one at a time against its owned state.

Across process boundaries, actions use stable action identifiers plus serializable arguments, not code addresses:

```forge
accounts.send_action(
    :deposit,
    #account/deposit {:amount amount}
)?;
```

Agents are a library model, not an object inheritance mechanism.

## ECS interaction

Large stores can be partitioned so one worker/agent owns a shard. Messages move operations or compact data to the shard, improving locality and reducing general locking.

## Closures in concurrency

Forge v1 ordinary closures are non-escaping. APIs that queue work for later execution must therefore accept either:

- a plain function pointer plus copied concrete arguments;
- a generated owned action record;
- or another explicitly owned callable representation supplied by a library.

The runtime never silently heap-boxes a closure environment.
