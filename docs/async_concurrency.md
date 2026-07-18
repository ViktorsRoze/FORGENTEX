# Deterministic async/concurrency runtime (slices 1–2)

Forgentex's concurrency foundation is a deterministic cooperative runtime for
typed tasks, bounded structured task groups, and bounded FIFO channels. It
establishes ownership, cancellation, capacity, and effect contracts before
introducing OS-thread scheduling or non-blocking asynchronous I/O.

## Task contract

`task_spawn(f, value)` accepts a typed one-argument `Fn[P, R]` or
`Fn[P, R, effects[E…]]`, moves the input into a `Task[R]`, and captures the
callable. The task remains pending until
`task_await(task)` executes it. A task handle is affine: awaiting or cancelling
it consumes the handle under the ownership checker, while dropping an unused
pending handle is still permitted in this slice.

`task_cancel(task)` cancels a pending task without executing it and returns
`true`. Cancelling a non-pending task returns `false`. Awaiting a cancelled or
already-consumed task traps deterministically.

Task operations infer the `concurrency.task` effect, which must be declared by
the containing function. A direct function's declared effects and a closure
body's inferred effects propagate to the spawning function. Indirect function
values propagate their annotated `effects[...]` set too, preventing a task from
hiding capabilities behind an opaque callable.

```fgx
fn double(value: i32) -> i32:
    return value * 2

fn calculate() -> i32 effects [concurrency.task]:
    let task: Task[i32] = task_spawn(double, 21)
    return task_await(task)
```

## Structured task-group contract

`task_group_new(capacity)` creates an affine `TaskGroup[T]` with a capacity in
the inclusive range `1..=1024`. All children in a group return the same `T`.
Child handles never escape the group.

- `task_group_spawn(group, f, value)` moves the input into a pending child and
  returns `true`. If the group is full, it evaluates and consumes the callable
  and input under normal eager call semantics, drops them without scheduling a
  child, and returns `false`.
- `task_group_await_all(group)` consumes the group, executes each pending child
  in insertion order, and returns `Vec[T]` in that same order.
- `task_group_cancel(group)` consumes the group, propagates cancellation to
  every pending child without running it, and returns the cancelled-child
  count.

Task-group operations and child callable effects follow the same
`concurrency.task` effect contract as standalone tasks.

```fgx
fn score(value: i32) -> i32:
    return value * 10

fn scores() -> i32 effects [concurrency.task]:
    let group: TaskGroup[i32] = task_group_new(3)
    let first: bool = task_group_spawn(group, score, 1)
    let second: bool = task_group_spawn(group, score, 2)
    let third: bool = task_group_spawn(group, score, 3)
    let values: Vec[i32] = task_group_await_all(group)
    return values[0] + values[1] + values[2]
```

## Channel contract

`channel_new(capacity)` creates a `Channel[T]` whose element type is fixed by
its annotated binding. Capacity must be greater than zero.

- `channel_send(channel, value)` appends in FIFO order and returns `true`.
- Send returns `false` immediately if the channel is full or closed.
- `channel_recv(channel)` returns `Some(value)` or `None` immediately.
- `channel_close(channel)` rejects future sends while preserving queued values.

Channel operations infer the `concurrency.channel` effect. There are no
unbounded queues and no implicit blocking behavior in this slice.

## Deliberate limits

The deterministic interpreter executes a task only at its await point and a
task group's children only at its ordered join. This is a cooperative reference
schedule, not parallel execution. These slices have no multi-argument tasks,
nested structured scopes, select, backpressure suspension, wakeups, timers,
async filesystem/network operations, worker threads, or runtime audit receipt.
Those features must preserve the affine handles, bounded capacity, explicit
effects, cancellation propagation, and deterministic test-mode contracts
introduced here.
