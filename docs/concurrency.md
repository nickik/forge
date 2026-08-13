# Forge v1 Concurrency Model

## Philosophy

Forge does not require green threads or a language scheduler. Concurrency builds on operating-system threads/processes, explicit synchronization, CSP channels, and optional data-owning agents.

## Threads

`std.thread` exposes OS-backed thread creation, join, yield/sleep and thread-local execution context. Targets without threads reject thread-dependent programs at compile/link configuration time rather than pretending to support them.

Each thread receives a derived `context`, including its own scratch arena unless explicitly overridden.

## Atomics

Provide concrete atomic scalar/pointer types in v1, e.g. `Atomic_u32`, with explicit relaxed/acquire/release/acq_rel/seq_cst ordering where supported.

A convenience `update` operation may apply a non-escaping function repeatedly using compare-exchange:

```forge
counter.update(
    [](x: u32) -> u32 { return x + 1; }
)?;
```

The update function may execute more than once and therefore must not perform externally visible side effects.

## CSP channels

Channels are typed standard-library objects. Implementation may use shared-memory queues, OS pipes/message primitives or sockets depending on endpoint kind.

Conceptual usage:

```forge
val jobs = channel.create_Job(
    :mode = :thread,
    :capacity = 256
)?;

jobs.send(job)?;
val next = jobs.recv()?;
```

`select` is language syntax because multi-channel waiting is sufficiently fundamental:

```forge
select {
    recv jobs -> job => {
        process(job);
    }

    recv shutdown -> _ => {
        return;
    }

    timeout #duration "100ms" => {
        maintenance();
    }
}
```

Across OS processes, values are copied/serialized or transferred through explicit shared-memory handles. Raw function pointers are never process-portable messages.

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
