# Deterministic async/concurrency runtime (slices 1–23)

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

`task_spawn_parked(f, value)` captures the same typed callable and owned input
but creates the task in an explicit parked state. `task_wake(task)` changes a
parked task to pending and returns `true`; waking an already-pending or closed
task returns `false`. Waking reads the affine handle without consuming it, so
the newly ready task can subsequently be awaited or cancelled. Awaiting a
parked task traps with `task_parked` instead of silently running it.

`task_cancel(task)` cancels a pending or parked task without executing it and
returns `true`. Cancelling a non-open task returns `false`. Awaiting a cancelled
or already-consumed task traps deterministically.

`task_then(task, continuation)` consumes `Task[T]` without running it, attaches
a typed one-argument `T -> U` callable, and returns `Task[U]`. Chains retain the
seed task's pending, explicitly parked, or wake-token-waiting state. Awaiting
the final handle executes the seed and then each continuation in attachment
order; an error stops the chain, while cancellation prevents every remaining
stage from running. Each chain admits at most 1,024 continuations and refuses
overflow with `task_continuation_capacity`.

The returned handle is the sole owner of the chain. The source handle is moved,
existing readiness observations of that source close, and callers may create a
new borrowed readiness arm for the returned task. Callable effects propagate at
attachment just as they do at spawn. This provides bounded deferred
continuation composition; an arbitrary running function still cannot suspend
its statement frame and later resume it.

`task_poll(task)` is the nonblocking scheduler boundary. It borrows an open
`Task[T]` and returns `None` without changing the task while it is explicitly
parked or waiting on an unsignaled token. Once runnable, polling executes the
seed and every attached continuation exactly once and returns `Some(value)`.
Wake-token registration remains allocated across a pending poll and is
released when a later poll observes the signal.

A completed or failed poll dynamically closes the task just like await; later
poll, wake, or await attempts trap or report closure. The ownership checker
keeps the handle available after a poll because `None` must be retryable, so
callers must treat `Some` as the terminal branch. This is deterministic
cooperative polling, not background execution or a suspended statement frame.

`task_wait_on(task, token)` consumes any open pending, explicitly parked, or
token-waiting task and returns a new affine handle gated by `token`. Rebinding
does not execute the seed or its continuation chain. An unsignaled token parks
the returned task; an already-signaled token makes it pending immediately.
`task_poll` can therefore observe `None`, the program can signal the gate, and
a later poll can complete the same deferred chain.

Rebinding a token-waiting task releases its old registration before installing
the new one, while capacity is preflighted so refusal leaves the old task and
registration unchanged at runtime. The new gate uses the same 1,024-waiter
ceiling. Cancellation releases the transferred registration, cancelled tokens
are refused, source readiness observations close, and the token remains a
borrowed copyable capability. This suspends deferred task execution at a call
boundary; it does not capture a partially executed callable frame.

`task_wait(clock, task)` is the consuming blocking form for one task. A
runnable task executes immediately. For a task gated by a logical timer on the
supplied clock, the runtime advances only to that deadline, fires every timer
due there in stable deadline/creation order, releases the selected timer slot,
and then executes the task and its continuation chain. A timer already made
ready by manual signal is disarmed without advancing logical time.

Timers from another clock trap with `task_wait_clock`. An explicitly parked
task or an unsignaled program-only token has no scheduler-owned progress source
and traps with `reactor_stalled` rather than hanging. `task_wait` consumes the
task, borrows the clock, and requires both `concurrency.task` and
`concurrency.timer`. It blocks only by deterministic logical-time advancement;
it never sleeps a host thread or reads wall time.

`task_race(clock, tasks)` consumes a non-empty `Vec[Task[T]]` and returns the
winning `T`. The input is capped at 1,024 distinct open tasks. The lowest
runnable input wins immediately; otherwise the supplied clock advances only to
the nearest timer backing one of the inputs, fires every due timer stably, and
the lowest newly ready task wins. The winner executes its complete continuation
chain, while every loser is cancelled without running. A winning timer already
made ready manually is disarmed without advancing logical time.

All timer-backed inputs must belong to the supplied clock. An empty vector,
closed or duplicate task, cross-clock timer, and over-capacity set trap before
selection mutates any task. If every input is explicitly parked or waiting on
an unsignaled program-only token, the race traps with `reactor_stalled` instead
of hanging. Cancelling losers releases their waiter registrations but does not
cancel the underlying copyable wake tokens or withdraw their timer deadlines;
token aliases therefore retain their independent one-shot meaning. The task
vector is moved, the clock is borrowed, and both task and timer effects are
required.

`task_timeout(clock, task, delay)` consumes one `Task[T]` and returns
`Option[T]`. A runnable or already-signaled task wins immediately. Otherwise a
same-clock task timer wins when its deadline is before or equal to the bounded
timeout deadline; the runtime advances there, fires every due timer stably,
and returns `Some(value)` after executing the complete continuation chain.

When the timeout deadline comes first, the runtime advances there, cancels the
task without running it, and returns `None`. This gives explicitly parked and
program-token-only tasks a deterministic terminal path rather than a stalled
wait. Timeout cancellation releases a wake registration but does not withdraw
a later task timer or cancel the independently aliased token. A zero-delay
timeout still lets work already ready at the current tick win. The deadline is
bounded by the 1,000,000,000-tick clock ceiling and does not reserve a timer
queue slot. Delay expressions are evaluated eagerly before readiness is
rechecked. Cross-clock timer sources reject before the task is changed.

Task operations infer the `concurrency.task` effect, which must be declared by
the containing function. A direct function's declared effects and a closure
body's inferred effects propagate to the spawning function. Indirect function
values propagate their annotated `effects[...]` set too, preventing a task from
hiding capabilities behind an opaque callable.

### Wake-token contract

`wake_token_new()` creates a copyable, shared, one-shot `WakeToken`. Copies
refer to the same signal state. `task_spawn_on(token, f, value)` captures typed
owned work like `task_spawn`, but parks it on the token while the token remains
unsignaled. Each token admits at most 1,024 parked waiters; overflow traps with
`wake_token_capacity` instead of allocating an unbounded wait list.

`wake_token_signal(token)` marks the token signaled and returns the number of
currently registered waiters. Repeated signals return zero. A registered task
observes the signal at its next await, records a deterministic event wakeup,
and then executes. Tasks spawned after the one-shot signal start pending
without registering. Cancelling or manually waking a waiting task releases its
registration. Callable/input evaluation remains eager, and token state is
rechecked afterward so a signaling input expression cannot create a stale
parked task.

`wake_token_select(arms)` consumes a `Vec[WakeToken]` of at most 1,024 distinct
tokens and performs a nonblocking readiness check. If no arm is ready it
returns `None` without changing any token. Otherwise the lowest ready input
index wins, the function returns `Some(index)`, and every losing one-shot arm
is cancelled. Losing logical timers are removed from their clock queues
immediately, and a selected timer that became ready through a manual signal is
disarmed too. Selection refuses tokens that are already cancelled or already
have parked task waiters, preventing cancellation from stranding tasks.

`wake_token_wait(clock, arms)` is the bounded blocking form for deterministic
reference execution. It reads an `AsyncClock`, consumes the same kind of token
vector, and returns the winning index directly. If an arm is already ready,
the lowest ready input wins without advancing time. Otherwise the runtime
advances only to the nearest selected-arm timer deadline, fires all timers due
on that clock in deadline/creation order, selects the lowest ready arm, and
cancels every loser. Timer arms must belong to the supplied clock. Program-
signaled tokens may also be arms, but a set with no ready arm or scheduled
timer traps with `reactor_stalled` instead of sleeping or hanging forever.

Waiting infers both `concurrency.task` and `concurrency.timer`. This is a
deterministic program-driven readiness loop, not a host reactor. Signaling does
not execute child code immediately. `task_then` chains deferred call
boundaries, but a running callable still cannot suspend its statement frame.

### Heterogeneous readiness contract

`readiness_arm_token(token)`, `readiness_arm_task(task)`,
`readiness_arm_channel(channel)`, and
`readiness_arm_channel_send(channel)` create borrowed in-memory
`ReadinessArm` observations. Hosted runs additionally support
`readiness_arm_file_read(path)` and `readiness_arm_file_create(path)` for a
borrowed `Path`.
The adapters read rather than consume their source handles, remain connected to
later task/channel state changes, and are moved into a bounded
`Vec[ReadinessArm]`. Task readiness means the task is runnable or its registered
wake source has fired; it does not mean that the callable ran in the
background. Channel read readiness means that a value is buffered or the
channel is closed, matching an immediate `channel_recv` outcome. Channel send
readiness means that the channel is open and below capacity, matching an
immediately successful `channel_send`. File-read availability means that the
capability-confined path currently names a regular file.
File-create availability means that a capability-confined destination is
absent and therefore eligible for no-clobber atomic publication.

`readiness_wait(clock, arms)` consumes at most 1,024 distinct arms and returns
the lowest ready input index. If none is ready, it advances only to the nearest
timer directly represented by a token arm or by a timer-waiting task arm. Due
timers fire in stable deadline/creation order before readiness is checked
again. All timer-derived arms must belong to the supplied clock. A set with no
ready or future source traps with `reactor_stalled`.

`readiness_wait_timeout(clock, arms, delay)` adds a bounded logical deadline
and returns `Option[usize]`. An already-ready arm wins as `Some(index)` without
advancing time. Otherwise a represented arm timer before or at the timeout
deadline fires first and the lowest ready arm wins. If the deadline comes
first, the runtime advances exactly there, fires every generally due clock
timer in stable order, and returns `None`.

Timeout preserves every observed token, task, channel, and file source; it does
not apply the ordinary winning selection's losing-token cancellation. The
borrowed task/channel handles therefore remain usable, and token aliases retain
their state. An empty arm vector is valid for this form and acts as deterministic
logical sleep while still firing due clock timers. The delay is eagerly
evaluated and range-checked against the logical clock ceiling. Equal deadlines
favor arm readiness. All existing duplicate, closed, waiter, cross-clock, and
1,024-arm validations still apply.

Read and send observations of one channel are distinct sources and may appear
together; duplicate observations of the same direction are rejected. Both
adapters remain connected to later send, receive, and close transitions.

The winning and losing task/channel handles remain available for subsequent
`task_await`, `task_cancel`, `channel_recv`, send, or close operations. Losing
token arms are cancelled and their timer slots reclaimed; task and channel
losers are only observed. A raw token with parked task waiters is rejected—use
`readiness_arm_task` so losing selection cannot strand a task. Duplicate source
arms and closed task handles are rejected. The unified wait conservatively
requires `concurrency.task`, `concurrency.channel`, and `concurrency.timer`.
The file adapters separately require `fs.read` or `fs.write`; they are
unavailable in the plain deterministic interpreter and denied when the hosted
run lacks the corresponding grant.
All `concurrency.*` effects remain interpreter-runtime effects; C and Zig source
lowering reject them explicitly until a native scheduler contract exists.

### Logical-clock and timer contract

`clock_new()` creates a copyable shared `AsyncClock` at logical tick zero.
`clock_now(clock)` observes its tick. `timer_after(clock, delay)` returns a
`WakeToken`: delay zero is ready immediately, while a positive delay occupies
one bounded timer slot until its deadline. Each clock holds at most 1,024
scheduled timers and cannot advance beyond tick 1,000,000,000.

`clock_advance(clock, ticks)` advances logical time explicitly, removes and
signals every due timer, and returns the number of due entries. Timers fire by
deadline and then creation order, so equal-deadline behavior is stable across
hosts. Their wake tokens compose directly with `task_spawn_on`. A token already
signaled manually records `already_signaled` when its timer becomes due.
Selection reclaims losing timer slots before their deadlines.

`timer_cancel(token)` withdraws a pending logical timer, returns `true`, marks
all token aliases cancelled, and releases the queue slot immediately. Repeated
cancellation or cancellation after firing returns `false`. Cancellation
refuses a timer with parked task waiters; callers must cancel or wake those
tasks first so no task can be stranded on a source that will never fire.

`timer_reschedule(clock, token, delay)` moves a pending timer on the same clock
without changing its token identity or waiter set. A zero delay makes it ready
immediately and releases its queue slot. A positive delay receives a fresh
scheduling sequence, so if it moves onto an occupied deadline it fires after
timers already scheduled there. Cross-clock moves, plain wake tokens, invalid
ranges, and deadlines beyond the logical tick ceiling trap deterministically;
fired or cancelled timers return `false`. Both operations borrow their handles
and infer `concurrency.timer`.

`timer_interval(clock, period)` creates a copyable shared `IntervalTimer` with
a positive fixed-rate period. `timer_interval_next(interval)` borrows that
handle, reserves the next cadence deadline, and returns a fresh ordinary
one-shot `WakeToken`. Multiple aliases advance one shared cadence. Future
deadlines occupy the clock's existing bounded timer queue and retain stable
deadline/creation ordering. If logical time has already passed one or more
cadence deadlines, each call exposes the next missed tick as an immediately
ready token until the interval catches up; no tick is silently coalesced.

`timer_interval_cancel(interval)` closes the shared handle and returns `true`;
repeated closure returns `false`, and requesting another tick traps with
`timer_interval_closed`. Tokens issued before closure remain independent and
still fire, and callers may cancel or reschedule those tokens through the
ordinary timer-control operations. Period zero, queue overflow, and cadence
deadlines beyond tick 1,000,000,000 trap deterministically. Once no further
deadline fits beneath that ceiling, `timer_interval_next` traps with
`timer_interval_exhausted`.

Clock and timer operations infer `concurrency.timer`. They never read host wall
time, sleep a thread, or execute tasks inline. External reactor integration
remains deferred.

```fgx
fn double(value: i32) -> i32:
    return value * 2

fn increment(value: i32) -> i32:
    return value + 1

fn calculate() -> i32 effects [concurrency.task]:
    let seed: Task[i32] = task_spawn_parked(double, 21)
    let task: Task[i32] = task_then(seed, increment)
    let woke: bool = task_wake(task)
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
  in insertion order, and returns `Vec[T]` in that same order. The join drains
  each owned child from the consumed scope before execution; completed payloads
  are not cloned or retained in the handle.
- `task_group_await_next(group)` executes and removes exactly one pending child
  in insertion order and returns `Some(value)`. An empty group returns `None`
  without consuming the group. Removing a child releases one capacity slot, so
  callers can bound in-flight work while incrementally spawning and joining.
- `task_group_spawn_wait(group, f, value)` applies bounded cooperative
  backpressure. While capacity is available it schedules the child and returns
  `None`. When the group is full, it executes and removes the oldest child,
  schedules the new child in the released slot, and returns the completed
  result as `Some(value)`. The callable and input are evaluated eagerly, the
  group never exceeds its capacity, and no child handle escapes. Capacity and
  scope state are rechecked after eager evaluation, so an input expression that
  itself advances the group cannot produce a stale admission decision.
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
unbounded queues. `readiness_arm_channel` adds buffered-or-closed read
readiness. `readiness_arm_channel_send` adds open-and-below-capacity send
readiness; filling the buffer disables it, receiving re-enables it, and closing
the channel disables it permanently. An empty open channel cannot acquire data
and a full open channel cannot gain capacity in the deterministic single-thread
schedule while a wait is active.

### Hosted filesystem availability

`readiness_arm_file_read(path)` validates the borrowed `Path` through the same
application confinement rules as ordinary reads. Existing paths are
canonicalized; a missing final component is admitted only when its canonical
parent lies under an `fs.read` root. The arm is ready only for a regular file.
A newly introduced symlink is not followed, preventing a post-adapter scope
escape.

Construction records a zero-byte `readiness_read` I/O operation and does not
consume the application's read-byte budget. The arm remains connected to the
path, so a regular file created by an intervening synchronous operation is
visible to the next readiness check. Missing files can be bounded with
`readiness_wait_timeout`; logical time advances to the deadline without host
polling. Equal canonical paths are duplicate sources even when supplied
through different bindings.

`readiness_arm_file_create(path)` applies the same confinement rules under
`fs.write`, but observes the create-new side of the contract. It is ready only
when the final destination component is absent, matching the no-clobber atomic
writers. Existing files, directories, and dangling symlinks are pending. An
intervening atomic publication therefore changes a pre-created arm from ready
to pending. Construction records a zero-byte `readiness_create` I/O operation;
it does not create a file or reserve write-budget capacity. Equal resolved
destinations are rejected as duplicates.

## Runtime receipt

Successful entry-point JSON includes a `forgentex.concurrency_receipt.v1`
object whenever concurrency primitives run. Its `deterministic_cooperative`
mode records a zero-based sequence of task, task-group, and channel
transitions. Each event contains the operation, source binding or callable,
decision, current child/queue count, and capacity when applicable.

The receipt includes accepted and refused decisions. A full group or channel
records `capacity_exhausted`; a send after close records `closed`; empty
receives, cancellation, ordered joins, and successful awaits are visible too.
Successful incremental joins record `task_group.await_next` with `completed`
or `empty` and the number of children still pending; a child failure traps
after the failed child has been removed from the scope.
Backpressured spawns record `task_group.spawn_wait` with `accepted` or
`capacity_released`; the count is the bounded number of pending children after
the new child is admitted.
Parked task creation records `task.spawn_parked` with `parked`. Explicit wake
attempts record `task.wake` with `woken`, `already_ready`, or `already_closed`,
making the scheduler-state transition visible without wall-clock data.
Wake tokens add `wake_token.new`, `task.spawn_on`, `wake_token.signal`,
`wake_token.select`, `reactor.wait`, `wake_token.wait`, `wake_token.cancel`, and
`task.wake_event`. Their counts expose bounded registrations, selected indices,
logical ticks advanced by a wait, loser cancellation, and ordered scheduler
observations; shared token aliases retain the same deterministic token identity
and state.
Deferred continuations add `task.then` and `task.resume`. Attachment events
expose bounded chain depth; resume events expose the stable stage index, total
continuation count, and completed/error decision.
Nonblocking polling adds `task.poll`, distinguishing `parked`, token `waiting`,
`completed`, and `error` outcomes without host timestamps.
Wake-source rebinding adds `task.wait_on`; its target records the moved task and
new token binding, while its decision and count expose ready/parked admission,
the new gate's waiter count, and bounded-capacity refusal.
Single-task blocking adds `task.wait` plus the existing `reactor.wait` and
`timer.fire` evidence, exposing immediate readiness, exact logical ticks
advanced, terminal completion/error, and stalled refusal.
Homogeneous consuming races add `task.race` and `task.cancel` loser evidence;
counts expose the stable winning input, capacity refusal, and loser indices.
Logical task deadlines add `task.timeout` and timeout-flavored `task.cancel`
evidence; the timeout count records the configured bounded delay while
`reactor.wait` records the exact logical progress.
Heterogeneous waits add `readiness.arm` and `readiness.wait`. They record each
borrowed source's observed readiness and the stable selected kind/binding;
channel targets distinguish `channel:<binding>` reads from
`channel_send:<binding>` writes, and token-only loser cancellation remains
visible through `wake_token.cancel`.
Hosted file targets use `file_read:<binding>` or `file_create:<binding>`; their
separate I/O transcript records the permission decision and zero content bytes.
Bounded heterogeneous deadlines add `readiness.wait_timeout`; selected counts
identify the winning arm, while timed-out counts expose the configured delay
and retain every source without cancellation evidence.
Logical timers add `clock.new`, `clock.now`, `timer.after`, `timer.fire`, and
`clock.advance`; timer control adds `timer.cancel` and `timer.reschedule`. Fire
events expose waiter counts; advance events summarize the number of due timers,
while cancellation/rescheduling events expose the stable token identity,
decision, remaining queue depth, and capacity.
Fixed-rate intervals add `timer.interval.open`, `timer.interval.next`, and
`timer.interval.cancel`; their events expose the stable interval identity,
period, scheduled-versus-ready decision, shared queue depth, and closure state.

No wall-clock timestamps or host identifiers enter the receipt, so identical
programs and inputs produce byte-stable event order across hosts. Programs
that use no concurrency primitives retain the previous run JSON shape.

## Deliberate limits

The deterministic interpreter executes a task only at its await point and a
task group's children only at an incremental or full ordered join. This is a
cooperative reference schedule, not parallel execution. These slices have no
multi-argument tasks, nested structured scopes, heterogeneous blocking select
with open-stream/socket readiness arms, statement-frame suspension and
resumption, OS/reactor-driven wake sources, wall-clock timers, async
filesystem/network operations, active task interruption, or worker threads.
Hosted file arms recheck regular-file or create-new availability only at
deterministic program wait boundaries; they do not monitor the filesystem while
blocked.
Parked, token-waiting, and logical-timer tasks model
pre-execution scheduler states; a running callable still executes to
completion. `task_poll` can defer that execution until a task is runnable, but
cannot yield from the middle of the callable.
Those features must preserve the affine handles, bounded capacity, explicit
effects, cancellation propagation, and deterministic test-mode contracts
introduced here.
