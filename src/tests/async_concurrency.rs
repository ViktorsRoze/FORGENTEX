use crate::format::{format_agent, format_human};
use crate::interp::{run_entry_json, run_entry_json_hosted};
use crate::parser::{parse_agent, parse_human};

const TASK_PROGRAM: &str = "fn double(x: i32) -> i32:\n    return x * 2\n\nfn run_task() -> i32 effects [concurrency.task]:\n    let task: Task[i32] = task_spawn(double, 21)\n    return task_await(task)\n\nfn cancel_task() -> bool effects [concurrency.task]:\n    let task: Task[i32] = task_spawn(double, 21)\n    return task_cancel(task)\n";

const PARKED_TASK_PROGRAM: &str = "fn double(x: i32) -> i32:\n    return x * 2\n\nfn run_woken() -> i32 effects [concurrency.task]:\n    let task: Task[i32] = task_spawn_parked(double, 21)\n    let first: bool = task_wake(task)\n    let second: bool = task_wake(task)\n    if !first || second:\n        return -1\n    return task_await(task)\n\nfn cancel_parked() -> bool effects [concurrency.task]:\n    let task: Task[i32] = task_spawn_parked(double, 21)\n    return task_cancel(task)\n";

const TASK_CONTINUATION_PROGRAM: &str = "fn double(value: i32) -> i32:\n    return value * 2\n\nfn increment(value: i32) -> i32:\n    return value + 1\n\nfn positive(value: i32) -> bool:\n    return value > 0\n\nfn chained() -> bool effects [concurrency.task]:\n    let seed: Task[i32] = task_spawn(double, 3)\n    let incremented: Task[i32] = task_then(seed, increment)\n    let result: Task[bool] = task_then(incremented, positive)\n    return task_await(result)\n\nfn parked_chain() -> i32 effects [concurrency.task]:\n    let seed: Task[i32] = task_spawn_parked(double, 4)\n    let result: Task[i32] = task_then(seed, increment)\n    let woke: bool = task_wake(result)\n    if !woke:\n        return -1\n    return task_await(result)\n\nfn token_chain() -> i32 effects [concurrency.task]:\n    let token: WakeToken = wake_token_new()\n    let seed: Task[i32] = task_spawn_on(token, double, 5)\n    let result: Task[i32] = task_then(seed, increment)\n    let signaled: usize = wake_token_signal(token)\n    if signaled != 1:\n        return -1\n    return task_await(result)\n";

const TASK_POLL_PROGRAM: &str = "fn double(value: i32) -> i32:\n    return value * 2\n\nfn increment(value: i32) -> i32:\n    return value + 1\n\nfn poll_pending() -> i32 effects [concurrency.task]:\n    let task: Task[i32] = task_spawn(double, 7)\n    return unwrap_or(task_poll(task), -1)\n\nfn poll_parked() -> i32 effects [concurrency.task]:\n    let task: Task[i32] = task_spawn_parked(double, 4)\n    let before: i32 = unwrap_or(task_poll(task), 3)\n    let woke: bool = task_wake(task)\n    let after: i32 = unwrap_or(task_poll(task), 0)\n    if !woke:\n        return -1\n    return before * 10 + after\n\nfn poll_token() -> i32 effects [concurrency.task]:\n    let token: WakeToken = wake_token_new()\n    let task: Task[i32] = task_spawn_on(token, double, 5)\n    let before: i32 = unwrap_or(task_poll(task), 4)\n    let signaled: usize = wake_token_signal(token)\n    let after: i32 = unwrap_or(task_poll(task), 0)\n    if signaled != 1:\n        return -1\n    return before * 100 + after\n\nfn poll_chain() -> i32 effects [concurrency.task]:\n    let seed: Task[i32] = task_spawn(double, 6)\n    let task: Task[i32] = task_then(seed, increment)\n    return unwrap_or(task_poll(task), -1)\n";

const TASK_WAIT_ON_PROGRAM: &str = "fn double(value: i32) -> i32:\n    return value * 2\n\nfn increment(value: i32) -> i32:\n    return value + 1\n\nfn gated_chain() -> i32 effects [concurrency.task]:\n    let token: WakeToken = wake_token_new()\n    let seed: Task[i32] = task_spawn(double, 4)\n    let chain: Task[i32] = task_then(seed, increment)\n    let task: Task[i32] = task_wait_on(chain, token)\n    let before: i32 = unwrap_or(task_poll(task), 3)\n    let signaled: usize = wake_token_signal(token)\n    let after: i32 = unwrap_or(task_poll(task), 0)\n    if signaled != 1:\n        return -1\n    return before * 10 + after\n\nfn transfer_gate() -> i32 effects [concurrency.task]:\n    let first: WakeToken = wake_token_new()\n    let second: WakeToken = wake_token_new()\n    let seed: Task[i32] = task_spawn_on(first, double, 5)\n    let task: Task[i32] = task_wait_on(seed, second)\n    let old_waiters: usize = wake_token_signal(first)\n    let before: i32 = unwrap_or(task_poll(task), 4)\n    let new_waiters: usize = wake_token_signal(second)\n    let after: i32 = unwrap_or(task_poll(task), 0)\n    return i32(old_waiters) * 1000 + before * 100 + i32(new_waiters) * 10 + after\n\nfn already_ready_gate() -> i32 effects [concurrency.task]:\n    let token: WakeToken = wake_token_new()\n    let signaled: usize = wake_token_signal(token)\n    let seed: Task[i32] = task_spawn_parked(double, 6)\n    let task: Task[i32] = task_wait_on(seed, token)\n    return unwrap_or(task_poll(task), -1)\n\nfn cancel_gate() -> usize effects [concurrency.task]:\n    let token: WakeToken = wake_token_new()\n    let seed: Task[i32] = task_spawn(double, 6)\n    let task: Task[i32] = task_wait_on(seed, token)\n    let cancelled: bool = task_cancel(task)\n    if !cancelled:\n        return 99\n    return wake_token_signal(token)\n";

const TASK_BLOCKING_WAIT_PROGRAM: &str = "fn double(value: i32) -> i32:\n    return value * 2\n\nfn increment(value: i32) -> i32:\n    return value + 1\n\nfn wait_for_timer_chain() -> i32 effects [concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let earlier: WakeToken = timer_after(clock, 2)\n    let gate: WakeToken = timer_after(clock, 5)\n    let seed: Task[i32] = task_spawn(double, 4)\n    let chain: Task[i32] = task_then(seed, increment)\n    let task: Task[i32] = task_wait_on(chain, gate)\n    let result: i32 = task_wait(clock, task)\n    return result * 100 + i32(clock_now(clock)) * 10 + i32(wake_token_signal(earlier))\n\nfn wait_ready() -> i32 effects [concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let task: Task[i32] = task_spawn(double, 3)\n    return task_wait(clock, task) * 10 + i32(clock_now(clock))\n\nfn manually_ready_timer() -> i32 effects [concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let token: WakeToken = timer_after(clock, 5)\n    let task: Task[i32] = task_spawn_on(token, increment, 6)\n    let signaled: usize = wake_token_signal(token)\n    let result: i32 = task_wait(clock, task)\n    return result * 10 + i32(clock_advance(clock, 5))\n";

const TASK_RACE_PROGRAM: &str = "fn double(value: i32) -> i32:\n    return value * 2\n\nfn increment(value: i32) -> i32:\n    return value + 1\n\nfn explode(value: i32) -> i32:\n    return 1 / value\n\nfn timer_winner() -> i32 effects [alloc, concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let slow_gate: WakeToken = timer_after(clock, 5)\n    let fast_gate: WakeToken = timer_after(clock, 2)\n    let slow: Task[i32] = task_spawn_on(slow_gate, double, 9)\n    let fast_seed: Task[i32] = task_spawn_on(fast_gate, double, 2)\n    let fast: Task[i32] = task_then(fast_seed, increment)\n    let tasks: Vec[Task[i32]] = vec_new()\n    push(tasks, slow)\n    push(tasks, fast)\n    let result: i32 = task_race(clock, tasks)\n    return result * 100 + i32(clock_now(clock)) * 10 + i32(clock_advance(clock, 3))\n\nfn ready_winner() -> i32 effects [alloc, concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let first: Task[i32] = task_spawn(double, 3)\n    let loser: Task[i32] = task_spawn(explode, 0)\n    let tasks: Vec[Task[i32]] = vec_new()\n    push(tasks, first)\n    push(tasks, loser)\n    return task_race(clock, tasks)\n\nfn shared_timer_uses_lowest_input() -> i32 effects [alloc, concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let gate: WakeToken = timer_after(clock, 3)\n    let first: Task[i32] = task_spawn_on(gate, increment, 10)\n    let second: Task[i32] = task_spawn_on(gate, double, 10)\n    let tasks: Vec[Task[i32]] = vec_new()\n    push(tasks, first)\n    push(tasks, second)\n    return task_race(clock, tasks) * 10 + i32(clock_now(clock))\n\nfn manually_ready_timer_is_disarmed() -> i32 effects [alloc, concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let gate: WakeToken = timer_after(clock, 5)\n    let task: Task[i32] = task_spawn_on(gate, increment, 6)\n    let signaled: usize = wake_token_signal(gate)\n    let tasks: Vec[Task[i32]] = vec_new()\n    push(tasks, task)\n    let result: i32 = task_race(clock, tasks)\n    return result * 10 + i32(clock_advance(clock, 5))\n";

const TASK_TIMEOUT_PROGRAM: &str = "fn double(value: i32) -> i32:\n    return value * 2\n\nfn increment(value: i32) -> i32:\n    return value + 1\n\nfn explode(value: i32) -> i32:\n    return 1 / value\n\nfn task_timer_wins() -> i32 effects [concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let gate: WakeToken = timer_after(clock, 2)\n    let seed: Task[i32] = task_spawn_on(gate, double, 4)\n    let task: Task[i32] = task_then(seed, increment)\n    let result: i32 = unwrap_or(task_timeout(clock, task, 5), 0)\n    return result * 100 + i32(clock_now(clock)) * 10 + i32(clock_advance(clock, 3))\n\nfn equal_deadline_task_wins() -> i32 effects [concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let gate: WakeToken = timer_after(clock, 3)\n    let task: Task[i32] = task_spawn_on(gate, increment, 5)\n    return unwrap_or(task_timeout(clock, task, 3), 0) * 10 + i32(clock_now(clock))\n\nfn parked_times_out() -> i32 effects [concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let task: Task[i32] = task_spawn_parked(explode, 0)\n    return unwrap_or(task_timeout(clock, task, 4), 7) * 10 + i32(clock_now(clock))\n\nfn program_token_times_out() -> i32 effects [concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let token: WakeToken = wake_token_new()\n    let task: Task[i32] = task_spawn_on(token, increment, 1)\n    let result: i32 = unwrap_or(task_timeout(clock, task, 2), 7)\n    return result * 100 + i32(clock_now(clock)) * 10 + i32(wake_token_signal(token))\n\nfn later_timer_survives_timeout() -> i32 effects [concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let gate: WakeToken = timer_after(clock, 5)\n    let task: Task[i32] = task_spawn_on(gate, increment, 1)\n    let result: i32 = unwrap_or(task_timeout(clock, task, 2), 0)\n    return result * 100 + i32(clock_now(clock)) * 10 + i32(clock_advance(clock, 3))\n\nfn zero_deadline_ready_task_wins() -> i32 effects [concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let task: Task[i32] = task_spawn(double, 3)\n    return unwrap_or(task_timeout(clock, task, 0), 0) * 10 + i32(clock_now(clock))\n\nfn eager_delay_progress_is_observed() -> i32 effects [concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let gate: WakeToken = timer_after(clock, 1)\n    let task: Task[i32] = task_spawn_on(gate, increment, 5)\n    return unwrap_or(task_timeout(clock, task, clock_advance(clock, 1)), 0) * 10 + i32(clock_now(clock))\n";

const WAKE_TOKEN_PROGRAM: &str = "fn identity_size(x: usize) -> usize:\n    return x\n\nfn broadcast() -> usize effects [concurrency.task]:\n    let token: WakeToken = wake_token_new()\n    let alias: WakeToken = token\n    let first: Task[usize] = task_spawn_on(token, identity_size, 10)\n    let second: Task[usize] = task_spawn_on(alias, identity_size, 20)\n    let awakened: usize = wake_token_signal(token)\n    let repeated: usize = wake_token_signal(alias)\n    let first_value: usize = task_await(first)\n    let second_value: usize = task_await(second)\n    return awakened * 1000 + repeated * 100 + first_value * 10 + second_value\n\nfn late_ready() -> usize effects [concurrency.task]:\n    let token: WakeToken = wake_token_new()\n    let awakened: usize = wake_token_signal(token)\n    let task: Task[usize] = task_spawn_on(token, identity_size, 7)\n    return awakened * 10 + task_await(task)\n";

const TIMER_PROGRAM: &str = "fn identity_tick(x: usize) -> usize:\n    return x\n\nfn ordered_timers() -> usize effects [concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let alias: AsyncClock = clock\n    let slow_token: WakeToken = timer_after(clock, 5)\n    let fast_token: WakeToken = timer_after(alias, 2)\n    let tie_token: WakeToken = timer_after(clock, 2)\n    let slow: Task[usize] = task_spawn_on(slow_token, identity_tick, 50)\n    let fast: Task[usize] = task_spawn_on(fast_token, identity_tick, 20)\n    let tie: Task[usize] = task_spawn_on(tie_token, identity_tick, 30)\n    let before: usize = clock_now(alias)\n    let first_fired: usize = clock_advance(clock, 2)\n    let fast_value: usize = task_await(fast)\n    let tie_value: usize = task_await(tie)\n    let second_fired: usize = clock_advance(alias, 3)\n    let slow_value: usize = task_await(slow)\n    let after: usize = clock_now(clock)\n    return before + first_fired * 100000 + fast_value * 1000 + tie_value * 10 + second_fired + slow_value + after\n\nfn immediate_timer() -> usize effects [concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let token: WakeToken = timer_after(clock, 0)\n    let task: Task[usize] = task_spawn_on(token, identity_tick, 7)\n    let fired: usize = clock_advance(clock, 0)\n    return fired * 10 + task_await(task)\n";

const TIMER_CONTROL_PROGRAM: &str = "fn identity(value: usize) -> usize:\n    return value\n\nfn cancel_alias() -> usize effects [concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let token: WakeToken = timer_after(clock, 5)\n    let alias: WakeToken = token\n    let cancelled: bool = timer_cancel(alias)\n    let repeated: bool = timer_cancel(token)\n    let fired: usize = clock_advance(clock, 5)\n    let signaled: usize = wake_token_signal(token)\n    if !cancelled || repeated:\n        return 99\n    return fired * 10 + signaled\n\nfn reschedule_tie() -> usize effects [alloc, concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let first: WakeToken = timer_after(clock, 3)\n    let moved: WakeToken = timer_after(clock, 1)\n    let changed: bool = timer_reschedule(clock, moved, 3)\n    let fired: usize = clock_advance(clock, 3)\n    let arms: Vec[WakeToken] = vec_new()\n    push(arms, moved)\n    push(arms, first)\n    let winner: usize = unwrap_or(wake_token_select(arms), 9)\n    if !changed:\n        return 99\n    return fired * 10 + winner\n\nfn reschedule_waiting_task() -> usize effects [concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let token: WakeToken = timer_after(clock, 5)\n    let task: Task[usize] = task_spawn_on(token, identity, 7)\n    let changed: bool = timer_reschedule(clock, token, 2)\n    let early: usize = clock_advance(clock, 1)\n    let fired: usize = clock_advance(clock, 1)\n    if !changed:\n        return 99\n    return early * 100 + fired * 10 + task_await(task)\n\nfn reschedule_ready_now() -> usize effects [concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let token: WakeToken = timer_after(clock, 5)\n    let task: Task[usize] = task_spawn_on(token, identity, 4)\n    let changed: bool = timer_reschedule(clock, token, 0)\n    let fired: usize = clock_advance(clock, 5)\n    if !changed:\n        return 99\n    return fired * 10 + task_await(task)\n";

const PERIODIC_TIMER_PROGRAM: &str = "fn fixed_rate() -> usize effects [concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let interval: IntervalTimer = timer_interval(clock, 2)\n    let alias: IntervalTimer = interval\n    let first: WakeToken = timer_interval_next(interval)\n    let second: WakeToken = timer_interval_next(alias)\n    let third: WakeToken = timer_interval_next(interval)\n    let first_fired: usize = clock_advance(clock, 2)\n    let rest_fired: usize = clock_advance(clock, 4)\n    return first_fired * 100 + rest_fired * 10 + clock_now(clock)\n\nfn catch_up() -> usize effects [concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let interval: IntervalTimer = timer_interval(clock, 2)\n    let advanced: usize = clock_advance(clock, 5)\n    let first: WakeToken = timer_interval_next(interval)\n    let second: WakeToken = timer_interval_next(interval)\n    let third: WakeToken = timer_interval_next(interval)\n    let first_signal: usize = wake_token_signal(first)\n    let second_signal: usize = wake_token_signal(second)\n    return first_signal * 100 + second_signal * 10 + clock_advance(clock, 1)\n\nfn close_alias() -> usize effects [concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let interval: IntervalTimer = timer_interval(clock, 2)\n    let alias: IntervalTimer = interval\n    let issued: WakeToken = timer_interval_next(interval)\n    let closed: bool = timer_interval_cancel(alias)\n    let repeated: bool = timer_interval_cancel(interval)\n    if !closed || repeated:\n        return 99\n    return clock_advance(clock, 2)\n";

const SELECT_PROGRAM: &str = "fn select_identity(x: usize) -> usize:\n    return x\n\nfn select_timer() -> usize effects [alloc, concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let slow: WakeToken = timer_after(clock, 5)\n    let fast: WakeToken = timer_after(clock, 2)\n    let arms: Vec[WakeToken] = vec_new()\n    push(arms, slow)\n    push(arms, fast)\n    let fired: usize = clock_advance(clock, 2)\n    let winner: usize = unwrap_or(wake_token_select(arms), 99)\n    let task: Task[usize] = task_spawn_on(fast, select_identity, 7)\n    let loser_signal: usize = wake_token_signal(slow)\n    let later_fired: usize = clock_advance(clock, 3)\n    return fired * 10000 + winner * 1000 + task_await(task) * 10 + loser_signal + later_fired\n\nfn pending_then_ready() -> usize effects [alloc, concurrency.task]:\n    let first: WakeToken = wake_token_new()\n    let second: WakeToken = wake_token_new()\n    let pending_arms: Vec[WakeToken] = vec_new()\n    push(pending_arms, first)\n    push(pending_arms, second)\n    let pending: usize = unwrap_or(wake_token_select(pending_arms), 9)\n    let awakened: usize = wake_token_signal(second)\n    let ready_arms: Vec[WakeToken] = vec_new()\n    push(ready_arms, first)\n    push(ready_arms, second)\n    let winner: usize = unwrap_or(wake_token_select(ready_arms), 9)\n    return pending * 10 + awakened * 2 + winner\n\nfn simultaneous_ready() -> usize effects [alloc, concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let first: WakeToken = timer_after(clock, 0)\n    let second: WakeToken = timer_after(clock, 0)\n    let arms: Vec[WakeToken] = vec_new()\n    push(arms, second)\n    push(arms, first)\n    return unwrap_or(wake_token_select(arms), 9)\n";

const MANUAL_READY_TIMER_PROGRAM: &str = "fn manually_ready_timer() -> usize effects [alloc, concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let token: WakeToken = timer_after(clock, 5)\n    let awakened: usize = wake_token_signal(token)\n    let arms: Vec[WakeToken] = vec_new()\n    push(arms, token)\n    let winner: usize = unwrap_or(wake_token_select(arms), 9)\n    return awakened + winner * 10 + clock_advance(clock, 5)\n";

const REACTOR_WAIT_PROGRAM: &str = "fn wait_for_nearest_timer() -> usize effects [alloc, concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let slow: WakeToken = timer_after(clock, 5)\n    let manual: WakeToken = wake_token_new()\n    let fast: WakeToken = timer_after(clock, 2)\n    let arms: Vec[WakeToken] = vec_new()\n    push(arms, slow)\n    push(arms, manual)\n    push(arms, fast)\n    let winner: usize = wake_token_wait(clock, arms)\n    let now: usize = clock_now(clock)\n    let later: usize = clock_advance(clock, 3)\n    return winner * 100 + now * 10 + later\n\nfn ready_without_advance() -> usize effects [alloc, concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let first: WakeToken = timer_after(clock, 0)\n    let second: WakeToken = timer_after(clock, 0)\n    let arms: Vec[WakeToken] = vec_new()\n    push(arms, second)\n    push(arms, first)\n    return wake_token_wait(clock, arms) * 10 + clock_now(clock)\n";

const HETEROGENEOUS_READINESS_PROGRAM: &str = "fn double(value: i32) -> i32:\n    return value * 2\n\nfn timer_wakes_task_before_timeout() -> i32 effects [alloc, concurrency.task, concurrency.channel, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let task_timer: WakeToken = timer_after(clock, 2)\n    let timeout: WakeToken = timer_after(clock, 5)\n    let task: Task[i32] = task_spawn_on(task_timer, double, 7)\n    let channel: Channel[i32] = channel_new(1)\n    let arms: Vec[ReadinessArm] = vec_new()\n    push(arms, readiness_arm_channel(channel))\n    push(arms, readiness_arm_task(task))\n    push(arms, readiness_arm_token(timeout))\n    let winner: usize = readiness_wait(clock, arms)\n    let result: i32 = task_await(task)\n    let sent: bool = channel_send(channel, 9)\n    let received: i32 = unwrap_or(channel_recv(channel), 0)\n    if !sent:\n        return -1\n    return i32(winner) * 100 + i32(clock_now(clock)) * 10 + result + received\n\nfn lowest_ready_kind_wins() -> i32 effects [alloc, concurrency.task, concurrency.channel, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let task: Task[i32] = task_spawn(double, 3)\n    let channel: Channel[i32] = channel_new(1)\n    let sent: bool = channel_send(channel, 8)\n    let arms: Vec[ReadinessArm] = vec_new()\n    push(arms, readiness_arm_task(task))\n    push(arms, readiness_arm_channel(channel))\n    let winner: usize = readiness_wait(clock, arms)\n    return i32(winner) * 100 + task_await(task) * 10 + unwrap_or(channel_recv(channel), 0)\n\nfn closed_channel_is_readable() -> i32 effects [alloc, concurrency.task, concurrency.channel, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let channel: Channel[i32] = channel_new(1)\n    channel_close(channel)\n    let arms: Vec[ReadinessArm] = vec_new()\n    push(arms, readiness_arm_channel(channel))\n    return i32(readiness_wait(clock, arms)) * 10 + unwrap_or(channel_recv(channel), 7)\n";

const READINESS_TIMEOUT_PROGRAM: &str = "fn double(value: i32) -> i32:\n    return value * 2\n\nfn timer_task_wins() -> i32 effects [alloc, concurrency.task, concurrency.channel, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let gate: WakeToken = timer_after(clock, 2)\n    let task: Task[i32] = task_spawn_on(gate, double, 7)\n    let channel: Channel[i32] = channel_new(1)\n    let arms: Vec[ReadinessArm] = vec_new()\n    push(arms, readiness_arm_channel(channel))\n    push(arms, readiness_arm_task(task))\n    let winner: usize = unwrap_or(readiness_wait_timeout(clock, arms, 5), 9)\n    return i32(winner) * 100 + task_await(task) * 10 + i32(clock_now(clock))\n\nfn equal_token_deadline_wins() -> i32 effects [alloc, concurrency.task, concurrency.channel, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let token: WakeToken = timer_after(clock, 3)\n    let arms: Vec[ReadinessArm] = vec_new()\n    push(arms, readiness_arm_token(token))\n    return i32(unwrap_or(readiness_wait_timeout(clock, arms, 3), 9)) * 10 + i32(clock_now(clock))\n\nfn timeout_preserves_handles() -> i32 effects [alloc, concurrency.task, concurrency.channel, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let token: WakeToken = wake_token_new()\n    let task: Task[i32] = task_spawn_parked(double, 4)\n    let channel: Channel[i32] = channel_new(1)\n    let arms: Vec[ReadinessArm] = vec_new()\n    push(arms, readiness_arm_token(token))\n    push(arms, readiness_arm_task(task))\n    push(arms, readiness_arm_channel(channel))\n    let winner: usize = unwrap_or(readiness_wait_timeout(clock, arms, 4), 9)\n    let signaled: usize = wake_token_signal(token)\n    let woke: bool = task_wake(task)\n    let sent: bool = channel_send(channel, 9)\n    if !woke || !sent:\n        return -1\n    return i32(winner) * 1000 + i32(clock_now(clock)) * 100 + task_await(task) * 10 + unwrap_or(channel_recv(channel), 0) + i32(signaled)\n\nfn empty_sleep_fires_timer() -> i32 effects [alloc, concurrency.task, concurrency.channel, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let token: WakeToken = timer_after(clock, 2)\n    let arms: Vec[ReadinessArm] = vec_new()\n    let winner: usize = unwrap_or(readiness_wait_timeout(clock, arms, 3), 7)\n    return i32(winner) * 100 + i32(clock_now(clock)) * 10 + i32(wake_token_signal(token))\n\nfn ready_task_cancels_losing_timer() -> i32 effects [alloc, concurrency.task, concurrency.channel, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let loser: WakeToken = timer_after(clock, 5)\n    let task: Task[i32] = task_spawn(double, 3)\n    let arms: Vec[ReadinessArm] = vec_new()\n    push(arms, readiness_arm_task(task))\n    push(arms, readiness_arm_token(loser))\n    let winner: usize = unwrap_or(readiness_wait_timeout(clock, arms, 10), 9)\n    return i32(winner) * 100 + task_await(task) * 10 + i32(clock_advance(clock, 5))\n";

const WRITABLE_READINESS_PROGRAM: &str = "fn initially_writable() -> usize effects [alloc, concurrency.task, concurrency.channel, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let channel: Channel[i32] = channel_new(1)\n    let arms: Vec[ReadinessArm] = vec_new()\n    push(arms, readiness_arm_channel_send(channel))\n    return readiness_wait(clock, arms)\n\nfn read_and_write_are_distinct() -> i32 effects [alloc, concurrency.task, concurrency.channel, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let channel: Channel[i32] = channel_new(2)\n    let empty_arms: Vec[ReadinessArm] = vec_new()\n    push(empty_arms, readiness_arm_channel(channel))\n    push(empty_arms, readiness_arm_channel_send(channel))\n    let empty_winner: usize = readiness_wait(clock, empty_arms)\n    let sent: bool = channel_send(channel, 7)\n    let ready_arms: Vec[ReadinessArm] = vec_new()\n    push(ready_arms, readiness_arm_channel(channel))\n    push(ready_arms, readiness_arm_channel_send(channel))\n    let ready_winner: usize = readiness_wait(clock, ready_arms)\n    if !sent:\n        return -1\n    return i32(empty_winner) * 10 + i32(ready_winner)\n\nfn tracks_capacity_changes() -> i32 effects [alloc, concurrency.task, concurrency.channel, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let channel: Channel[i32] = channel_new(1)\n    let stalled: ReadinessArm = readiness_arm_channel_send(channel)\n    let resumed: ReadinessArm = readiness_arm_channel_send(channel)\n    let sent: bool = channel_send(channel, 7)\n    let stalled_arms: Vec[ReadinessArm] = vec_new()\n    push(stalled_arms, stalled)\n    let timed_out: usize = unwrap_or(readiness_wait_timeout(clock, stalled_arms, 2), 9)\n    let received: i32 = unwrap_or(channel_recv(channel), 0)\n    let resumed_arms: Vec[ReadinessArm] = vec_new()\n    push(resumed_arms, resumed)\n    let winner: usize = readiness_wait(clock, resumed_arms)\n    if !sent:\n        return -1\n    return i32(timed_out) * 100 + i32(clock_now(clock)) * 10 + received + i32(winner)\n\nfn close_disables_writes() -> i32 effects [alloc, concurrency.task, concurrency.channel, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let channel: Channel[i32] = channel_new(1)\n    let arm: ReadinessArm = readiness_arm_channel_send(channel)\n    channel_close(channel)\n    let arms: Vec[ReadinessArm] = vec_new()\n    push(arms, arm)\n    let winner: usize = unwrap_or(readiness_wait_timeout(clock, arms, 1), 8)\n    let sent: bool = channel_send(channel, 3)\n    if sent:\n        return -1\n    return i32(winner) * 10 + i32(clock_now(clock))\n";

const FILE_READINESS_PROGRAM: &str = "fn existing(path: Path) -> usize effects [alloc, concurrency.task, concurrency.channel, concurrency.timer, fs.read]:\n    let clock: AsyncClock = clock_new()\n    let arms: Vec[ReadinessArm] = vec_new()\n    push(arms, readiness_arm_file_read(path))\n    return readiness_wait(clock, arms)\n\nfn missing(path: Path) -> i32 effects [alloc, concurrency.task, concurrency.channel, concurrency.timer, fs.read]:\n    let clock: AsyncClock = clock_new()\n    let arms: Vec[ReadinessArm] = vec_new()\n    push(arms, readiness_arm_file_read(path))\n    let winner: usize = unwrap_or(readiness_wait_timeout(clock, arms, 2), 9)\n    return i32(winner) * 10 + i32(clock_now(clock))\n\nfn appears(path: Path) -> i32 effects [alloc, concurrency.task, concurrency.channel, concurrency.timer, fs.read, fs.write]:\n    let clock: AsyncClock = clock_new()\n    let arm: ReadinessArm = readiness_arm_file_read(path)\n    let written: usize = core_io_write_file_atomic(path, \"ready\")\n    let arms: Vec[ReadinessArm] = vec_new()\n    push(arms, arm)\n    return i32(written) * 10 + i32(readiness_wait(clock, arms))\n";

const FILE_CREATE_READINESS_PROGRAM: &str = "fn available(path: Path) -> usize effects [alloc, concurrency.task, concurrency.channel, concurrency.timer, fs.write]:\n    let clock: AsyncClock = clock_new()\n    let arms: Vec[ReadinessArm] = vec_new()\n    push(arms, readiness_arm_file_create(path))\n    return readiness_wait(clock, arms)\n\nfn existing(path: Path) -> i32 effects [alloc, concurrency.task, concurrency.channel, concurrency.timer, fs.write]:\n    let clock: AsyncClock = clock_new()\n    let arms: Vec[ReadinessArm] = vec_new()\n    push(arms, readiness_arm_file_create(path))\n    let winner: usize = unwrap_or(readiness_wait_timeout(clock, arms, 1), 8)\n    return i32(winner) * 10 + i32(clock_now(clock))\n\nfn claimed(path: Path) -> i32 effects [alloc, concurrency.task, concurrency.channel, concurrency.timer, fs.write]:\n    let clock: AsyncClock = clock_new()\n    let arm: ReadinessArm = readiness_arm_file_create(path)\n    let written: usize = core_io_write_file_atomic(path, \"ready\")\n    let arms: Vec[ReadinessArm] = vec_new()\n    push(arms, arm)\n    let winner: usize = unwrap_or(readiness_wait_timeout(clock, arms, 2), 9)\n    return i32(written) * 100 + i32(winner) * 10 + i32(clock_now(clock))\n";

const CHANNEL_PROGRAM: &str = "fn bounded_fifo() -> i32 effects [concurrency.channel]:\n    let channel: Channel[i32] = channel_new(2)\n    let first_sent: bool = channel_send(channel, 10)\n    let second_sent: bool = channel_send(channel, 20)\n    let overflow_sent: bool = channel_send(channel, 30)\n    if !first_sent || !second_sent || overflow_sent:\n        return -1\n    let first: i32 = unwrap_or(channel_recv(channel), 0)\n    let second: i32 = unwrap_or(channel_recv(channel), 0)\n    return first * 10 + second\n\nfn close_rejects_send() -> bool effects [concurrency.channel]:\n    let channel: Channel[i32] = channel_new(1)\n    channel_close(channel)\n    return channel_send(channel, 1)\n";

const TASK_GROUP_PROGRAM: &str = "fn identity(x: i32) -> i32:\n    return x\n\nfn ordered_group() -> i32 effects [concurrency.task]:\n    let group: TaskGroup[i32] = task_group_new(3)\n    let first: bool = task_group_spawn(group, identity, 1)\n    let second: bool = task_group_spawn(group, identity, 2)\n    let third: bool = task_group_spawn(group, identity, 3)\n    if !first || !second || !third:\n        return -1\n    let results: Vec[i32] = task_group_await_all(group)\n    return results[0] * 100 + results[1] * 10 + results[2]\n\nfn bounded_group() -> i32 effects [concurrency.task]:\n    let group: TaskGroup[i32] = task_group_new(1)\n    let first: bool = task_group_spawn(group, identity, 7)\n    let overflow: bool = task_group_spawn(group, identity, 9)\n    let results: Vec[i32] = task_group_await_all(group)\n    if !first || overflow:\n        return -1\n    return results[0]\n";

const BACKPRESSURE_PROGRAM: &str = "fn identity(x: i32) -> i32:\n    return x\n\nfn rolling_group() -> i32 effects [concurrency.task]:\n    let group: TaskGroup[i32] = task_group_new(2)\n    let first: i32 = unwrap_or(task_group_spawn_wait(group, identity, 1), 0)\n    let second: i32 = unwrap_or(task_group_spawn_wait(group, identity, 2), 0)\n    let released: i32 = unwrap_or(task_group_spawn_wait(group, identity, 3), 0)\n    let remaining: Vec[i32] = task_group_await_all(group)\n    return first + second + released * 100 + remaining[0] * 10 + remaining[1]\n";

fn expect_return(source: &str, entry: &str, expected: &str) {
    let program = parse_human(source).unwrap();
    let output = run_entry_json(
        &program,
        entry,
        &format!("{{ \"args\": {{}}, \"expect\": {{ \"return\": {expected} }} }}"),
    )
    .unwrap();
    assert!(output.contains("\"status\": \"pass\""), "{output}");
}

#[test]
fn deterministic_task_runs_only_when_awaited_and_returns_its_typed_value() {
    let program = parse_human(TASK_PROGRAM).unwrap();
    let output = run_entry_json(
        &program,
        "run_task",
        "{ \"args\": {}, \"expect\": { \"return\": 42 } }",
    )
    .unwrap();
    assert!(output.contains("\"operation\":\"task.spawn\""), "{output}");
    assert!(
        output.contains(
            "\"operation\":\"task.await\",\"target\":\"task\",\"decision\":\"completed\""
        ),
        "{output}"
    );
}

#[test]
fn pending_task_can_be_cancelled_without_running() {
    let program = parse_human(TASK_PROGRAM).unwrap();
    let output = run_entry_json(
        &program,
        "cancel_task",
        "{ \"args\": {}, \"expect\": { \"return\": true } }",
    )
    .unwrap();
    assert!(
        output.contains(
            "\"operation\":\"task.cancel\",\"target\":\"task\",\"decision\":\"cancelled\""
        ),
        "{output}"
    );
}

#[test]
fn parked_task_requires_an_explicit_wakeup_before_execution() {
    let program = parse_human(PARKED_TASK_PROGRAM).unwrap();
    let output = run_entry_json(
        &program,
        "run_woken",
        "{ \"args\": {}, \"expect\": { \"return\": 42 } }",
    )
    .unwrap();
    for transition in [
        "\"operation\":\"task.spawn_parked\",\"target\":\"double\",\"decision\":\"parked\"",
        "\"operation\":\"task.wake\",\"target\":\"task\",\"decision\":\"woken\"",
        "\"operation\":\"task.wake\",\"target\":\"task\",\"decision\":\"already_ready\"",
        "\"operation\":\"task.await\",\"target\":\"task\",\"decision\":\"completed\"",
    ] {
        assert!(
            output.contains(transition),
            "missing {transition}: {output}"
        );
    }
}

#[test]
fn awaiting_a_parked_task_traps_without_running_it() {
    let source = "fn identity(x: i32) -> i32:\n    return x\n\nfn blocked() -> i32 effects [concurrency.task]:\n    let task: Task[i32] = task_spawn_parked(identity, 7)\n    return task_await(task)\n";
    let program = parse_human(source).unwrap();
    let error = run_entry_json(&program, "blocked", "{ \"args\": {} }").unwrap_err();
    assert!(error.contains("task_parked"), "got: {error}");
}

#[test]
fn parked_task_can_be_cancelled_without_becoming_ready() {
    let program = parse_human(PARKED_TASK_PROGRAM).unwrap();
    let output = run_entry_json(
        &program,
        "cancel_parked",
        "{ \"args\": {}, \"expect\": { \"return\": true } }",
    )
    .unwrap();
    assert!(
        output.contains(
            "\"operation\":\"task.cancel\",\"target\":\"task\",\"decision\":\"cancelled\""
        ),
        "{output}"
    );
}

#[test]
fn task_continuations_chain_typed_results_without_running_at_attachment() {
    let program = parse_human(TASK_CONTINUATION_PROGRAM).unwrap();
    let output = run_entry_json(
        &program,
        "chained",
        "{ \"args\": {}, \"expect\": { \"return\": true } }",
    )
    .unwrap();
    for transition in [
        "\"operation\":\"task.then\",\"target\":\"seed\",\"decision\":\"chained\",\"count\":1",
        "\"operation\":\"task.then\",\"target\":\"incremented\",\"decision\":\"chained\",\"count\":2",
        "\"operation\":\"task.resume\",\"target\":\"result\",\"decision\":\"completed\",\"count\":1,\"capacity\":2",
        "\"operation\":\"task.resume\",\"target\":\"result\",\"decision\":\"completed\",\"count\":2,\"capacity\":2",
    ] {
        assert!(output.contains(transition), "missing {transition}: {output}");
    }
}

#[test]
fn continuation_chains_preserve_parked_and_token_waiting_state() {
    expect_return(TASK_CONTINUATION_PROGRAM, "parked_chain", "9");
    expect_return(TASK_CONTINUATION_PROGRAM, "token_chain", "11");
}

#[test]
fn cancelling_a_continuation_chain_prevents_every_stage_from_running() {
    let source = "fn explode(value: i32) -> i32:\n    return value / 0\n\nfn cancel_chain() -> bool effects [concurrency.task]:\n    let seed: Task[i32] = task_spawn(explode, 1)\n    let result: Task[i32] = task_then(seed, explode)\n    return task_cancel(result)\n";
    expect_return(source, "cancel_chain", "true");
}

#[test]
fn task_then_is_typed_effectful_and_consumes_the_source_handle() {
    let wrong_type = "fn size(value: str) -> usize:\n    return value.len\n\nfn wrong() -> usize effects [concurrency.task]:\n    let seed: Task[i32] = task_spawn(|value: i32| value, 1)\n    let result: Task[usize] = task_then(seed, size)\n    return task_await(result)\n";
    let diagnostics = crate::typecheck::check(&parse_human(wrong_type).unwrap());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "task_continuation_input_type"),
        "got: {diagnostics:?}"
    );

    let reused = "fn identity(value: i32) -> i32:\n    return value\n\nfn reused() -> i32 effects [concurrency.task]:\n    let seed: Task[i32] = task_spawn(identity, 1)\n    let result: Task[i32] = task_then(seed, identity)\n    return task_await(seed) + task_await(result)\n";
    let diagnostics = crate::typecheck::check(&parse_human(reused).unwrap());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "use_after_move"),
        "got: {diagnostics:?}"
    );

    let affine_callable = "fn identity(value: i32) -> i32:\n    return value\n\nfn reused_once() -> i32 effects [concurrency.task]:\n    let once: FnOnce[i32, i32] = |value: i32| value + 1\n    let seed: Task[i32] = task_spawn(identity, 1)\n    let result: Task[i32] = task_then(seed, once)\n    return once(2) + task_await(result)\n";
    let diagnostics = crate::typecheck::check(&parse_human(affine_callable).unwrap());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "use_after_move"),
        "got: {diagnostics:?}"
    );

    let missing_effect = "fn identity(value: i32) -> i32:\n    return value\n\nfn missing() -> i32:\n    let seed: Task[i32] = task_spawn(identity, 1)\n    let result: Task[i32] = task_then(seed, identity)\n    return task_await(result)\n";
    let diagnostics = crate::typecheck::check(&parse_human(missing_effect).unwrap());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "undeclared_effect"),
        "got: {diagnostics:?}"
    );
}

#[test]
fn task_then_propagates_continuation_effects() {
    let source = "fn identity(value: i32) -> i32:\n    return value\n\nfn read_next(value: i32) -> i32 effects [io.read]:\n    return value + 1\n\nfn missing_io() -> i32 effects [concurrency.task]:\n    let seed: Task[i32] = task_spawn(identity, 1)\n    let result: Task[i32] = task_then(seed, read_next)\n    return task_await(result)\n";
    let diagnostics = crate::typecheck::check(&parse_human(source).unwrap());
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "undeclared_effect" && diagnostic.message.contains("io.read")
        }),
        "got: {diagnostics:?}"
    );
}

#[test]
fn task_continuation_depth_is_hard_bounded() {
    let mut source = String::from(
        "fn identity(value: i32) -> i32:\n    return value\n\nfn overflow() -> i32 effects [concurrency.task]:\n    let task0: Task[i32] = task_spawn(identity, 1)\n",
    );
    for index in 0..=1024 {
        source.push_str(&format!(
            "    let task{}: Task[i32] = task_then(task{}, identity)\n",
            index + 1,
            index
        ));
    }
    source.push_str("    return task_await(task1025)\n");
    let program = parse_human(&source).unwrap();
    let error = run_entry_json(&program, "overflow", "{ \"args\": {} }").unwrap_err();
    assert!(error.contains("task_continuation_capacity"), "got: {error}");
}

#[test]
fn task_poll_executes_runnable_tasks_and_continuation_chains_once() {
    expect_return(TASK_POLL_PROGRAM, "poll_pending", "14");
    expect_return(TASK_POLL_PROGRAM, "poll_chain", "13");
}

#[test]
fn task_poll_preserves_parked_and_waiting_tasks_for_later_progress() {
    expect_return(TASK_POLL_PROGRAM, "poll_parked", "38");
    expect_return(TASK_POLL_PROGRAM, "poll_token", "410");
}

#[test]
fn task_poll_receipts_distinguish_pending_sources_from_completion() {
    let program = parse_human(TASK_POLL_PROGRAM).unwrap();
    let output = run_entry_json(
        &program,
        "poll_token",
        "{ \"args\": {}, \"expect\": { \"return\": 410 } }",
    )
    .unwrap();
    for transition in [
        "\"operation\":\"task.poll\",\"target\":\"task\",\"decision\":\"waiting\"",
        "\"operation\":\"task.wake_event\",\"target\":\"task\",\"decision\":\"woken\"",
        "\"operation\":\"task.poll\",\"target\":\"task\",\"decision\":\"completed\"",
    ] {
        assert!(
            output.contains(transition),
            "missing {transition}: {output}"
        );
    }
}

#[test]
fn completed_or_failed_task_poll_closes_the_runtime_handle() {
    let completed = "fn identity(value: i32) -> i32:\n    return value\n\nfn twice() -> i32 effects [concurrency.task]:\n    let task: Task[i32] = task_spawn(identity, 1)\n    let first: i32 = unwrap_or(task_poll(task), 0)\n    return unwrap_or(task_poll(task), first)\n";
    let program = parse_human(completed).unwrap();
    let error = run_entry_json(&program, "twice", "{ \"args\": {} }").unwrap_err();
    assert!(error.contains("task_already_consumed"), "got: {error}");

    let failed = "fn explode(value: i32) -> i32:\n    return value / 0\n\nfn fail() -> i32 effects [concurrency.task]:\n    let task: Task[i32] = task_spawn(explode, 1)\n    return unwrap_or(task_poll(task), 0)\n";
    let program = parse_human(failed).unwrap();
    let error = run_entry_json(&program, "fail", "{ \"args\": {} }").unwrap_err();
    assert!(error.contains("division_by_zero"), "got: {error}");
}

#[test]
fn task_poll_is_typed_effectful_and_borrows_its_handle() {
    let wrong = "fn wrong() -> Option[i32] effects [concurrency.task]:\n    return task_poll(1)\n";
    let diagnostics = crate::typecheck::check(&parse_human(wrong).unwrap());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "concurrency_handle_type"),
        "got: {diagnostics:?}"
    );

    let missing = "fn identity(value: i32) -> i32:\n    return value\n\nfn missing() -> i32:\n    let task: Task[i32] = task_spawn(identity, 1)\n    return unwrap_or(task_poll(task), 0)\n";
    let diagnostics = crate::typecheck::check(&parse_human(missing).unwrap());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "undeclared_effect"),
        "got: {diagnostics:?}"
    );

    let borrowed = "fn identity(value: i32) -> i32:\n    return value\n\nfn borrowed() -> bool effects [concurrency.task]:\n    let task: Task[i32] = task_spawn_parked(identity, 1)\n    let pending: Option[i32] = task_poll(task)\n    return task_cancel(task)\n";
    let diagnostics = crate::typecheck::check(&parse_human(borrowed).unwrap());
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "use_after_move"),
        "got: {diagnostics:?}"
    );
}

#[test]
fn task_wait_on_gates_a_deferred_continuation_chain_until_signal() {
    expect_return(TASK_WAIT_ON_PROGRAM, "gated_chain", "39");
    expect_return(TASK_WAIT_ON_PROGRAM, "already_ready_gate", "12");
}

#[test]
fn task_wait_on_transfers_an_existing_wake_registration_atomically() {
    expect_return(TASK_WAIT_ON_PROGRAM, "transfer_gate", "420");
    expect_return(TASK_WAIT_ON_PROGRAM, "cancel_gate", "0");
}

#[test]
fn task_wait_on_receipt_exposes_the_source_and_new_gate() {
    let program = parse_human(TASK_WAIT_ON_PROGRAM).unwrap();
    let output = run_entry_json(
        &program,
        "gated_chain",
        "{ \"args\": {}, \"expect\": { \"return\": 39 } }",
    )
    .unwrap();
    assert!(
        output.contains(
            "\"operation\":\"task.wait_on\",\"target\":\"chain@token\",\"decision\":\"parked\",\"count\":1,\"capacity\":1024"
        ),
        "{output}"
    );
}

#[test]
fn task_wait_on_is_typed_effectful_moves_the_task_and_borrows_the_token() {
    let wrong_task = "fn wrong() -> Task[i32] effects [concurrency.task]:\n    let token: WakeToken = wake_token_new()\n    return task_wait_on(1, token)\n";
    let wrong_token = "fn identity(value: i32) -> i32:\n    return value\n\nfn wrong() -> Task[i32] effects [concurrency.task]:\n    let task: Task[i32] = task_spawn(identity, 1)\n    return task_wait_on(task, 2)\n";
    for source in [wrong_task, wrong_token] {
        let diagnostics = crate::typecheck::check(&parse_human(source).unwrap());
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "concurrency_handle_type"),
            "got: {diagnostics:?}"
        );
    }

    let ownership = "fn identity(value: i32) -> i32:\n    return value\n\nfn moved() -> i32 effects [concurrency.task]:\n    let token: WakeToken = wake_token_new()\n    let seed: Task[i32] = task_spawn(identity, 1)\n    let task: Task[i32] = task_wait_on(seed, token)\n    let signaled: usize = wake_token_signal(token)\n    return task_await(seed) + task_await(task)\n";
    let diagnostics = crate::typecheck::check(&parse_human(ownership).unwrap());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "use_after_move"),
        "got: {diagnostics:?}"
    );
    assert!(
        diagnostics.iter().all(|diagnostic| {
            diagnostic.code != "use_after_move" || !diagnostic.message.contains("token")
        }),
        "token must remain borrowed: {diagnostics:?}"
    );

    let missing = "fn identity(value: i32) -> i32:\n    return value\n\nfn missing() -> i32:\n    let token: WakeToken = wake_token_new()\n    let seed: Task[i32] = task_spawn(identity, 1)\n    let task: Task[i32] = task_wait_on(seed, token)\n    return 0\n";
    let diagnostics = crate::typecheck::check(&parse_human(missing).unwrap());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "undeclared_effect"),
        "got: {diagnostics:?}"
    );
}

#[test]
fn task_wait_on_rejects_cancelled_tokens_before_runtime_rebinding() {
    let source = "fn identity(value: i32) -> i32:\n    return value\n\nfn rejected() -> Task[i32] effects [alloc, concurrency.task]:\n    let ready: WakeToken = wake_token_new()\n    let token: WakeToken = wake_token_new()\n    let awakened: usize = wake_token_signal(ready)\n    let arms: Vec[WakeToken] = vec_new()\n    push(arms, ready)\n    push(arms, token)\n    let selected: usize = unwrap_or(wake_token_select(arms), 9)\n    let task: Task[i32] = task_spawn(identity, 1)\n    return task_wait_on(task, token)\n";
    let program = parse_human(source).unwrap();
    let error = run_entry_json(&program, "rejected", "{ \"args\": {} }").unwrap_err();
    assert!(error.contains("wake_token_cancelled"), "got: {error}");
}

#[test]
fn task_wait_advances_to_its_timer_gate_and_runs_the_chain() {
    expect_return(TASK_BLOCKING_WAIT_PROGRAM, "wait_for_timer_chain", "950");
    expect_return(TASK_BLOCKING_WAIT_PROGRAM, "wait_ready", "60");
    expect_return(TASK_BLOCKING_WAIT_PROGRAM, "manually_ready_timer", "70");
}

#[test]
fn task_wait_receipts_expose_timer_progress_and_terminal_completion() {
    let program = parse_human(TASK_BLOCKING_WAIT_PROGRAM).unwrap();
    let output = run_entry_json(
        &program,
        "wait_for_timer_chain",
        "{ \"args\": {}, \"expect\": { \"return\": 950 } }",
    )
    .unwrap();
    for transition in [
        "\"operation\":\"timer.fire\",\"target\":\"0\",\"decision\":\"signaled\"",
        "\"operation\":\"timer.fire\",\"target\":\"1\",\"decision\":\"signaled\"",
        "\"operation\":\"reactor.wait\",\"target\":\"clock\",\"decision\":\"advanced\",\"count\":5",
        "\"operation\":\"task.wait\",\"target\":\"task\",\"decision\":\"completed\"",
    ] {
        assert!(
            output.contains(transition),
            "missing {transition}: {output}"
        );
    }
    assert_eq!(
        output.matches("\"operation\":\"reactor.wait\"").count(),
        1,
        "blocking one task should record one reactor decision: {output}"
    );
}

#[test]
fn task_wait_traps_for_unrepresented_or_cross_clock_sources() {
    let stalled = "fn identity(value: i32) -> i32:\n    return value\n\nfn stalled() -> i32 effects [concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let token: WakeToken = wake_token_new()\n    let task: Task[i32] = task_spawn_on(token, identity, 1)\n    return task_wait(clock, task)\n";
    let parked = "fn identity(value: i32) -> i32:\n    return value\n\nfn stalled() -> i32 effects [concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let task: Task[i32] = task_spawn_parked(identity, 1)\n    return task_wait(clock, task)\n";
    let cross_clock = "fn identity(value: i32) -> i32:\n    return value\n\nfn mismatched() -> i32 effects [concurrency.task, concurrency.timer]:\n    let first: AsyncClock = clock_new()\n    let second: AsyncClock = clock_new()\n    let token: WakeToken = timer_after(first, 1)\n    let task: Task[i32] = task_spawn_on(token, identity, 1)\n    return task_wait(second, task)\n";
    for (source, entry, expected) in [
        (stalled, "stalled", "reactor_stalled"),
        (parked, "stalled", "reactor_stalled"),
        (cross_clock, "mismatched", "task_wait_clock"),
    ] {
        let program = parse_human(source).unwrap();
        let error = run_entry_json(&program, entry, "{ \"args\": {} }").unwrap_err();
        assert!(error.contains(expected), "got: {error}");
    }
}

#[test]
fn task_wait_is_typed_effectful_consumes_the_task_and_borrows_the_clock() {
    let wrong_clock = "fn identity(value: i32) -> i32:\n    return value\n\nfn wrong() -> i32 effects [concurrency.task, concurrency.timer]:\n    let task: Task[i32] = task_spawn(identity, 1)\n    return task_wait(1, task)\n";
    let wrong_task = "fn wrong() -> i32 effects [concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    return task_wait(clock, 1)\n";
    for source in [wrong_clock, wrong_task] {
        let diagnostics = crate::typecheck::check(&parse_human(source).unwrap());
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "concurrency_handle_type"),
            "got: {diagnostics:?}"
        );
    }

    for effects in ["concurrency.task", "concurrency.timer"] {
        let source = format!(
            "fn identity(value: i32) -> i32:\n    return value\n\nfn missing() -> i32 effects [{effects}]:\n    let clock: AsyncClock = clock_new()\n    let task: Task[i32] = task_spawn(identity, 1)\n    return task_wait(clock, task)\n"
        );
        let diagnostics = crate::typecheck::check(&parse_human(&source).unwrap());
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "undeclared_effect"),
            "got: {diagnostics:?}"
        );
    }

    let ownership = "fn identity(value: i32) -> i32:\n    return value\n\nfn moved() -> i32 effects [concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let task: Task[i32] = task_spawn(identity, 1)\n    let result: i32 = task_wait(clock, task)\n    return task_await(task) + result + i32(clock_now(clock))\n";
    let diagnostics = crate::typecheck::check(&parse_human(ownership).unwrap());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "use_after_move"),
        "got: {diagnostics:?}"
    );
    assert!(
        diagnostics.iter().all(|diagnostic| {
            diagnostic.code != "use_after_move" || !diagnostic.message.contains("clock")
        }),
        "clock must remain borrowed: {diagnostics:?}"
    );
}

#[test]
fn task_race_selects_the_lowest_ready_task_or_nearest_timer_winner() {
    expect_return(TASK_RACE_PROGRAM, "timer_winner", "521");
    expect_return(TASK_RACE_PROGRAM, "ready_winner", "6");
    expect_return(TASK_RACE_PROGRAM, "shared_timer_uses_lowest_input", "113");
    expect_return(TASK_RACE_PROGRAM, "manually_ready_timer_is_disarmed", "70");
}

#[test]
fn task_race_receipts_expose_progress_winner_and_loser_cancellation() {
    let program = parse_human(TASK_RACE_PROGRAM).unwrap();
    let output = run_entry_json(
        &program,
        "timer_winner",
        "{ \"args\": {}, \"expect\": { \"return\": 521 } }",
    )
    .unwrap();
    for transition in [
        "\"operation\":\"timer.fire\",\"target\":\"1\",\"decision\":\"signaled\"",
        "\"operation\":\"reactor.wait\",\"target\":\"clock\",\"decision\":\"advanced\",\"count\":2",
        "\"operation\":\"task.cancel\",\"target\":\"tasks[0]\",\"decision\":\"race_loser\"",
        "\"operation\":\"task.race\",\"target\":\"tasks\",\"decision\":\"completed\",\"count\":1",
    ] {
        assert!(
            output.contains(transition),
            "missing {transition}: {output}"
        );
    }
    assert_eq!(
        output.matches("\"operation\":\"reactor.wait\"").count(),
        1,
        "one race should record one reactor decision: {output}"
    );
}

#[test]
fn task_race_traps_for_empty_stalled_and_cross_clock_inputs() {
    let empty = "fn empty() -> i32 effects [alloc, concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let tasks: Vec[Task[i32]] = vec_new()\n    return task_race(clock, tasks)\n";
    let parked = "fn identity(value: i32) -> i32:\n    return value\n\nfn stalled() -> i32 effects [alloc, concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let task: Task[i32] = task_spawn_parked(identity, 1)\n    let tasks: Vec[Task[i32]] = vec_new()\n    push(tasks, task)\n    return task_race(clock, tasks)\n";
    let program_token = "fn identity(value: i32) -> i32:\n    return value\n\nfn stalled() -> i32 effects [alloc, concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let token: WakeToken = wake_token_new()\n    let task: Task[i32] = task_spawn_on(token, identity, 1)\n    let tasks: Vec[Task[i32]] = vec_new()\n    push(tasks, task)\n    return task_race(clock, tasks)\n";
    let cross_clock = "fn identity(value: i32) -> i32:\n    return value\n\nfn mismatched() -> i32 effects [alloc, concurrency.task, concurrency.timer]:\n    let first: AsyncClock = clock_new()\n    let second: AsyncClock = clock_new()\n    let token: WakeToken = timer_after(first, 1)\n    let task: Task[i32] = task_spawn_on(token, identity, 1)\n    let tasks: Vec[Task[i32]] = vec_new()\n    push(tasks, task)\n    return task_race(second, tasks)\n";
    for (source, entry, expected) in [
        (empty, "empty", "task_race_empty"),
        (parked, "stalled", "reactor_stalled"),
        (program_token, "stalled", "reactor_stalled"),
        (cross_clock, "mismatched", "task_race_clock"),
    ] {
        let program = parse_human(source).unwrap();
        let error = run_entry_json(&program, entry, "{ \"args\": {} }").unwrap_err();
        assert!(error.contains(expected), "got: {error}");
    }
}

#[test]
fn task_race_enforces_its_bounded_input_set() {
    let mut source = String::from(
        "fn identity(value: i32) -> i32:\n    return value\n\nfn overflow() -> i32 effects [alloc, concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let tasks: Vec[Task[i32]] = vec_new()\n",
    );
    for index in 0..1025 {
        source.push_str(&format!(
            "    let task_{index}: Task[i32] = task_spawn(identity, {index})\n    push(tasks, task_{index})\n"
        ));
    }
    source.push_str("    return task_race(clock, tasks)\n");
    let program = parse_human(&source).unwrap();
    let error = run_entry_json(&program, "overflow", "{ \"args\": {} }").unwrap_err();
    assert!(error.contains("task_race_capacity"), "got: {error}");
}

#[test]
fn task_race_is_typed_effectful_moves_the_vector_and_borrows_the_clock() {
    let wrong_clock = "fn wrong() -> i32 effects [alloc, concurrency.task, concurrency.timer]:\n    let tasks: Vec[Task[i32]] = vec_new()\n    return task_race(1, tasks)\n";
    let wrong_tasks = "fn wrong() -> i32 effects [alloc, concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let values: Vec[i32] = vec_new()\n    return task_race(clock, values)\n";
    for source in [wrong_clock, wrong_tasks] {
        let diagnostics = crate::typecheck::check(&parse_human(source).unwrap());
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "concurrency_handle_type"),
            "got: {diagnostics:?}"
        );
    }

    for effects in ["alloc, concurrency.task", "alloc, concurrency.timer"] {
        let source = format!(
            "fn identity(value: i32) -> i32:\n    return value\n\nfn missing() -> i32 effects [{effects}]:\n    let clock: AsyncClock = clock_new()\n    let task: Task[i32] = task_spawn(identity, 1)\n    let tasks: Vec[Task[i32]] = vec_new()\n    push(tasks, task)\n    return task_race(clock, tasks)\n"
        );
        let diagnostics = crate::typecheck::check(&parse_human(&source).unwrap());
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "undeclared_effect"),
            "got: {diagnostics:?}"
        );
    }

    let ownership = "fn identity(value: i32) -> i32:\n    return value\n\nfn moved() -> i32 effects [alloc, concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let task: Task[i32] = task_spawn(identity, 1)\n    let tasks: Vec[Task[i32]] = vec_new()\n    push(tasks, task)\n    let result: i32 = task_race(clock, tasks)\n    return result + i32(tasks.len) + i32(clock_now(clock))\n";
    let diagnostics = crate::typecheck::check(&parse_human(ownership).unwrap());
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "use_after_move" && diagnostic.message.contains("tasks")
        }),
        "got: {diagnostics:?}"
    );
    assert!(
        diagnostics.iter().all(|diagnostic| {
            diagnostic.code != "use_after_move" || !diagnostic.message.contains("clock")
        }),
        "clock must remain borrowed: {diagnostics:?}"
    );
}

#[test]
fn task_timeout_selects_ready_timer_and_deadline_outcomes() {
    expect_return(TASK_TIMEOUT_PROGRAM, "task_timer_wins", "920");
    expect_return(TASK_TIMEOUT_PROGRAM, "equal_deadline_task_wins", "63");
    expect_return(TASK_TIMEOUT_PROGRAM, "parked_times_out", "74");
    expect_return(TASK_TIMEOUT_PROGRAM, "program_token_times_out", "720");
    expect_return(TASK_TIMEOUT_PROGRAM, "later_timer_survives_timeout", "21");
    expect_return(TASK_TIMEOUT_PROGRAM, "zero_deadline_ready_task_wins", "60");
    expect_return(
        TASK_TIMEOUT_PROGRAM,
        "eager_delay_progress_is_observed",
        "61",
    );
}

#[test]
fn task_timeout_receipts_distinguish_completion_from_cancellation() {
    let program = parse_human(TASK_TIMEOUT_PROGRAM).unwrap();
    let completed = run_entry_json(
        &program,
        "task_timer_wins",
        "{ \"args\": {}, \"expect\": { \"return\": 920 } }",
    )
    .unwrap();
    for transition in [
        "\"operation\":\"timer.fire\",\"target\":\"0\",\"decision\":\"signaled\"",
        "\"operation\":\"reactor.wait\",\"target\":\"clock\",\"decision\":\"advanced\",\"count\":2",
        "\"operation\":\"task.timeout\",\"target\":\"task\",\"decision\":\"completed\",\"count\":5",
    ] {
        assert!(
            completed.contains(transition),
            "missing {transition}: {completed}"
        );
    }

    let timed_out = run_entry_json(
        &program,
        "parked_times_out",
        "{ \"args\": {}, \"expect\": { \"return\": 74 } }",
    )
    .unwrap();
    for transition in [
        "\"operation\":\"reactor.wait\",\"target\":\"clock\",\"decision\":\"advanced\",\"count\":4",
        "\"operation\":\"task.cancel\",\"target\":\"task\",\"decision\":\"timeout\"",
        "\"operation\":\"task.timeout\",\"target\":\"task\",\"decision\":\"timed_out\",\"count\":4",
    ] {
        assert!(
            timed_out.contains(transition),
            "missing {transition}: {timed_out}"
        );
    }
}

#[test]
fn task_timeout_rejects_cross_clock_range_and_closed_tasks() {
    let cross_clock = "fn identity(value: i32) -> i32:\n    return value\n\nfn mismatched() -> Option[i32] effects [concurrency.task, concurrency.timer]:\n    let first: AsyncClock = clock_new()\n    let second: AsyncClock = clock_new()\n    let gate: WakeToken = timer_after(first, 1)\n    let task: Task[i32] = task_spawn_on(gate, identity, 1)\n    return task_timeout(second, task, 0)\n";
    let out_of_range = "fn identity(value: i32) -> i32:\n    return value\n\nfn bad() -> Option[i32] effects [concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let task: Task[i32] = task_spawn(identity, 1)\n    return task_timeout(clock, task, 1000000001)\n";
    let clock_overflow = "fn identity(value: i32) -> i32:\n    return value\n\nfn bad() -> Option[i32] effects [concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let advanced: usize = clock_advance(clock, 1000000000)\n    let task: Task[i32] = task_spawn_parked(identity, 1)\n    return task_timeout(clock, task, 1)\n";
    let closed = "fn identity(value: i32) -> i32:\n    return value\n\nfn bad() -> Option[i32] effects [concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let task: Task[i32] = task_spawn(identity, 1)\n    let result: i32 = unwrap_or(task_poll(task), 0)\n    return task_timeout(clock, task, 1)\n";
    for (source, entry, expected) in [
        (cross_clock, "mismatched", "task_timeout_clock"),
        (out_of_range, "bad", "task_timeout_range"),
        (clock_overflow, "bad", "clock_range"),
        (closed, "bad", "task_already_consumed"),
    ] {
        let program = parse_human(source).unwrap();
        let error = run_entry_json(&program, entry, "{ \"args\": {} }").unwrap_err();
        assert!(error.contains(expected), "got: {error}");
    }
}

#[test]
fn task_timeout_is_typed_effectful_consumes_the_task_and_borrows_the_clock() {
    let wrong_clock = "fn identity(value: i32) -> i32:\n    return value\n\nfn wrong() -> Option[i32] effects [concurrency.task, concurrency.timer]:\n    let task: Task[i32] = task_spawn(identity, 1)\n    return task_timeout(1, task, 1)\n";
    let wrong_task = "fn wrong() -> Option[i32] effects [concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    return task_timeout(clock, 1, 1)\n";
    for source in [wrong_clock, wrong_task] {
        let diagnostics = crate::typecheck::check(&parse_human(source).unwrap());
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "concurrency_handle_type"),
            "got: {diagnostics:?}"
        );
    }
    let wrong_delay = "fn identity(value: i32) -> i32:\n    return value\n\nfn wrong() -> Option[i32] effects [concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let task: Task[i32] = task_spawn(identity, 1)\n    return task_timeout(clock, task, \"later\")\n";
    let diagnostics = crate::typecheck::check(&parse_human(wrong_delay).unwrap());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "clock_tick_type"),
        "got: {diagnostics:?}"
    );

    for effects in ["concurrency.task", "concurrency.timer"] {
        let source = format!(
            "fn identity(value: i32) -> i32:\n    return value\n\nfn missing() -> Option[i32] effects [{effects}]:\n    let clock: AsyncClock = clock_new()\n    let task: Task[i32] = task_spawn(identity, 1)\n    return task_timeout(clock, task, 1)\n"
        );
        let diagnostics = crate::typecheck::check(&parse_human(&source).unwrap());
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "undeclared_effect"),
            "got: {diagnostics:?}"
        );
    }

    let ownership = "fn identity(value: i32) -> i32:\n    return value\n\nfn moved() -> i32 effects [concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let task: Task[i32] = task_spawn(identity, 1)\n    let result: i32 = unwrap_or(task_timeout(clock, task, 1), 0)\n    return task_await(task) + result + i32(clock_now(clock))\n";
    let diagnostics = crate::typecheck::check(&parse_human(ownership).unwrap());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "use_after_move"),
        "got: {diagnostics:?}"
    );
    assert!(
        diagnostics.iter().all(|diagnostic| {
            diagnostic.code != "use_after_move" || !diagnostic.message.contains("clock")
        }),
        "clock must remain borrowed: {diagnostics:?}"
    );
}

#[test]
fn wake_token_broadcast_releases_waiters_in_observation_order() {
    expect_return(WAKE_TOKEN_PROGRAM, "broadcast", "2120");
    let program = parse_human(WAKE_TOKEN_PROGRAM).unwrap();
    let output = run_entry_json(
        &program,
        "broadcast",
        "{ \"args\": {}, \"expect\": { \"return\": 2120 } }",
    )
    .unwrap();
    for transition in [
        "\"operation\":\"task.spawn_on\",\"target\":\"token\",\"decision\":\"parked\",\"count\":1,\"capacity\":1024",
        "\"operation\":\"task.spawn_on\",\"target\":\"alias\",\"decision\":\"parked\",\"count\":2,\"capacity\":1024",
        "\"operation\":\"wake_token.signal\",\"target\":\"token\",\"decision\":\"signaled\",\"count\":2,\"capacity\":1024",
        "\"operation\":\"wake_token.signal\",\"target\":\"alias\",\"decision\":\"already_signaled\",\"count\":2,\"capacity\":1024",
        "\"operation\":\"task.wake_event\",\"target\":\"first\",\"decision\":\"woken\",\"count\":1,\"capacity\":1024",
        "\"operation\":\"task.wake_event\",\"target\":\"second\",\"decision\":\"woken\",\"count\":0,\"capacity\":1024",
    ] {
        assert!(output.contains(transition), "missing {transition}: {output}");
    }
}

#[test]
fn task_spawned_after_signal_starts_ready_without_registering() {
    expect_return(WAKE_TOKEN_PROGRAM, "late_ready", "7");
    let program = parse_human(WAKE_TOKEN_PROGRAM).unwrap();
    let output = run_entry_json(
        &program,
        "late_ready",
        "{ \"args\": {}, \"expect\": { \"return\": 7 } }",
    )
    .unwrap();
    assert!(
        output.contains("\"operation\":\"task.spawn_on\",\"target\":\"token\",\"decision\":\"ready\",\"count\":0,\"capacity\":1024"),
        "{output}"
    );
    assert!(
        !output.contains("\"operation\":\"task.wake_event\""),
        "{output}"
    );
}

#[test]
fn unsignaled_wait_token_prevents_task_execution() {
    let source = "fn identity(x: i32) -> i32:\n    return x\n\nfn blocked() -> i32 effects [concurrency.task]:\n    let token: WakeToken = wake_token_new()\n    let task: Task[i32] = task_spawn_on(token, identity, 7)\n    return task_await(task)\n";
    let program = parse_human(source).unwrap();
    let error = run_entry_json(&program, "blocked", "{ \"args\": {} }").unwrap_err();
    assert!(error.contains("task_waiting"), "got: {error}");
}

#[test]
fn cancellation_releases_a_wake_registration() {
    let source = "fn identity(x: i32) -> i32:\n    return x\n\nfn cancel_waiter() -> usize effects [concurrency.task]:\n    let token: WakeToken = wake_token_new()\n    let task: Task[i32] = task_spawn_on(token, identity, 7)\n    let cancelled: bool = task_cancel(task)\n    if !cancelled:\n        return 99\n    return wake_token_signal(token)\n";
    expect_return(source, "cancel_waiter", "0");
}

#[test]
fn wake_token_state_is_rechecked_after_eager_input_evaluation() {
    let source = "fn identity(x: usize) -> usize:\n    return x\n\nfn reentrant_signal() -> usize effects [concurrency.task]:\n    let token: WakeToken = wake_token_new()\n    let task: Task[usize] = task_spawn_on(token, identity, wake_token_signal(token))\n    return task_await(task)\n";
    let program = parse_human(source).unwrap();
    let output = run_entry_json(
        &program,
        "reentrant_signal",
        "{ \"args\": {}, \"expect\": { \"return\": 0 } }",
    )
    .unwrap();
    assert!(
        output.contains("\"operation\":\"task.spawn_on\",\"target\":\"token\",\"decision\":\"ready\",\"count\":0,\"capacity\":1024"),
        "{output}"
    );
}

#[test]
fn wake_token_waiter_count_is_hard_bounded() {
    let mut source = String::from(
        "fn identity(x: i32) -> i32:\n    return x\n\nfn overflow() -> i32 effects [concurrency.task]:\n    let token: WakeToken = wake_token_new()\n",
    );
    for index in 0..=1024 {
        source.push_str(&format!(
            "    let task{index}: Task[i32] = task_spawn_on(token, identity, {index})\n"
        ));
    }
    source.push_str("    return 0\n");
    let program = parse_human(&source).unwrap();
    let error = run_entry_json(&program, "overflow", "{ \"args\": {} }").unwrap_err();
    assert!(error.contains("wake_token_capacity"), "got: {error}");
}

#[test]
fn wake_token_builtin_type_name_cannot_be_shadowed() {
    for source in [
        "struct WakeToken:\n    value: i32\n\nfn demo() -> i32:\n    let value: WakeToken = WakeToken(1)\n    return value.value\n",
        "enum WakeToken:\n    Ready\n\nfn demo() -> bool:\n    return true\n",
        "struct AsyncClock:\n    tick: usize\n\nfn demo() -> usize:\n    let value: AsyncClock = AsyncClock(1)\n    return value.tick\n",
        "struct ReadinessArm:\n    ready: bool\n\nfn demo() -> bool:\n    let value: ReadinessArm = ReadinessArm(true)\n    return value.ready\n",
    ] {
        let diagnostics = crate::typecheck::check(&parse_human(source).unwrap());
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "reserved_type_name"),
            "got: {diagnostics:?}"
        );
    }
}

#[test]
fn deterministic_clock_fires_deadlines_and_fifo_ties_in_order() {
    expect_return(TIMER_PROGRAM, "ordered_timers", "220356");
    let program = parse_human(TIMER_PROGRAM).unwrap();
    let output = run_entry_json(
        &program,
        "ordered_timers",
        "{ \"args\": {}, \"expect\": { \"return\": 220356 } }",
    )
    .unwrap();
    let first = output
        .find("\"operation\":\"timer.fire\",\"target\":\"1\"")
        .unwrap();
    let tie = output
        .find("\"operation\":\"timer.fire\",\"target\":\"2\"")
        .unwrap();
    let slow = output
        .find("\"operation\":\"timer.fire\",\"target\":\"0\"")
        .unwrap();
    assert!(
        first < tie && tie < slow,
        "timer order was not stable: {output}"
    );
    assert!(
        output.contains("\"operation\":\"clock.advance\",\"target\":\"clock\",\"decision\":\"advanced\",\"count\":2,\"capacity\":1024"),
        "{output}"
    );
    assert!(
        output.contains("\"operation\":\"clock.advance\",\"target\":\"alias\",\"decision\":\"advanced\",\"count\":1,\"capacity\":1024"),
        "{output}"
    );
}

#[test]
fn zero_delay_timer_is_ready_without_consuming_queue_capacity() {
    expect_return(TIMER_PROGRAM, "immediate_timer", "7");
    let program = parse_human(TIMER_PROGRAM).unwrap();
    let output = run_entry_json(
        &program,
        "immediate_timer",
        "{ \"args\": {}, \"expect\": { \"return\": 7 } }",
    )
    .unwrap();
    assert!(
        output.contains("\"operation\":\"timer.after\",\"target\":\"clock\",\"decision\":\"ready\",\"count\":0,\"capacity\":1024"),
        "{output}"
    );
}

#[test]
fn timer_cancel_is_alias_visible_idempotent_and_reclaims_its_slot() {
    expect_return(TIMER_CONTROL_PROGRAM, "cancel_alias", "0");
    let program = parse_human(TIMER_CONTROL_PROGRAM).unwrap();
    let output = run_entry_json(
        &program,
        "cancel_alias",
        "{ \"args\": {}, \"expect\": { \"return\": 0 } }",
    )
    .unwrap();
    for transition in [
        "\"operation\":\"timer.cancel\",\"target\":\"0\",\"decision\":\"cancelled\",\"count\":0,\"capacity\":1024",
        "\"operation\":\"timer.cancel\",\"target\":\"0\",\"decision\":\"already_cancelled\",\"count\":0,\"capacity\":1024",
        "\"operation\":\"clock.advance\",\"target\":\"clock\",\"decision\":\"advanced\",\"count\":0,\"capacity\":1024",
        "\"operation\":\"wake_token.signal\",\"target\":\"token\",\"decision\":\"cancelled\"",
    ] {
        assert!(output.contains(transition), "missing {transition}: {output}");
    }

    let mut source = String::from(
        "fn reclaimed() -> usize effects [concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n",
    );
    for index in 0..1024 {
        source.push_str(&format!(
            "    let token{index}: WakeToken = timer_after(clock, 1)\n"
        ));
    }
    source.push_str("    let cancelled: bool = timer_cancel(token0)\n    let replacement: WakeToken = timer_after(clock, 1)\n    return clock_advance(clock, 1)\n");
    expect_return(&source, "reclaimed", "1024");
}

#[test]
fn timer_reschedule_uses_a_fresh_stable_tie_sequence() {
    expect_return(TIMER_CONTROL_PROGRAM, "reschedule_tie", "20");
    let program = parse_human(TIMER_CONTROL_PROGRAM).unwrap();
    let output = run_entry_json(
        &program,
        "reschedule_tie",
        "{ \"args\": {}, \"expect\": { \"return\": 20 } }",
    )
    .unwrap();
    assert!(
        output.contains("\"operation\":\"timer.reschedule\",\"target\":\"1\",\"decision\":\"rescheduled\",\"count\":2,\"capacity\":1024"),
        "{output}"
    );
    let first = output
        .find("\"operation\":\"timer.fire\",\"target\":\"0\"")
        .unwrap();
    let moved = output
        .find("\"operation\":\"timer.fire\",\"target\":\"1\"")
        .unwrap();
    assert!(
        first < moved,
        "rescheduled tie order was unstable: {output}"
    );
}

#[test]
fn timer_reschedule_moves_waiting_tasks_and_can_make_them_ready_now() {
    expect_return(TIMER_CONTROL_PROGRAM, "reschedule_waiting_task", "17");
    expect_return(TIMER_CONTROL_PROGRAM, "reschedule_ready_now", "4");
    let program = parse_human(TIMER_CONTROL_PROGRAM).unwrap();
    let output = run_entry_json(
        &program,
        "reschedule_ready_now",
        "{ \"args\": {}, \"expect\": { \"return\": 4 } }",
    )
    .unwrap();
    assert!(
        output.contains("\"operation\":\"timer.reschedule\",\"target\":\"0\",\"decision\":\"ready\",\"count\":0,\"capacity\":1024"),
        "{output}"
    );
}

#[test]
fn timer_cancel_refuses_to_strand_registered_tasks() {
    let source = "fn identity(value: usize) -> usize:\n    return value\n\nfn unsafe_cancel() -> bool effects [concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let token: WakeToken = timer_after(clock, 1)\n    let task: Task[usize] = task_spawn_on(token, identity, 7)\n    return timer_cancel(token)\n";
    let program = parse_human(source).unwrap();
    let error = run_entry_json(&program, "unsafe_cancel", "{ \"args\": {} }").unwrap_err();
    assert!(error.contains("timer_cancel_waiters"), "got: {error}");
}

#[test]
fn timer_control_rejects_plain_tokens_cross_clock_moves_and_closed_timers() {
    for (source, entry, expected) in [
        (
            "fn plain() -> bool effects [concurrency.task, concurrency.timer]:\n    let token: WakeToken = wake_token_new()\n    return timer_cancel(token)\n",
            "plain",
            "timer_source",
        ),
        (
            "fn cross_clock() -> bool effects [concurrency.task, concurrency.timer]:\n    let first: AsyncClock = clock_new()\n    let second: AsyncClock = clock_new()\n    let token: WakeToken = timer_after(first, 1)\n    return timer_reschedule(second, token, 2)\n",
            "cross_clock",
            "timer_reschedule_clock",
        ),
    ] {
        let program = parse_human(source).unwrap();
        let error = run_entry_json(&program, entry, "{ \"args\": {} }").unwrap_err();
        assert!(error.contains(expected), "got: {error}");
    }

    let fired = "fn fired() -> usize effects [concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let token: WakeToken = timer_after(clock, 1)\n    let count: usize = clock_advance(clock, 1)\n    let cancelled: bool = timer_cancel(token)\n    let changed: bool = timer_reschedule(clock, token, 1)\n    if cancelled || changed:\n        return 99\n    return count\n";
    expect_return(fired, "fired", "1");
}

#[test]
fn timer_control_is_typed_effectful_and_borrows_clock_and_token() {
    for source in [
        "fn wrong() -> bool effects [concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    return timer_cancel(clock)\n",
        "fn wrong() -> bool effects [concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let token: WakeToken = timer_after(clock, 1)\n    return timer_reschedule(token, token, 1)\n",
        "fn wrong() -> bool effects [concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let token: WakeToken = timer_after(clock, 1)\n    return timer_reschedule(clock, token, true)\n",
    ] {
        let diagnostics = crate::typecheck::check(&parse_human(source).unwrap());
        assert!(
            diagnostics.iter().any(|diagnostic| {
                matches!(
                    diagnostic.code.as_str(),
                    "concurrency_handle_type" | "timer_delay_type"
                )
            }),
            "got: {diagnostics:?}"
        );
    }

    let missing = "fn missing() -> bool:\n    let clock: AsyncClock = clock_new()\n    let token: WakeToken = timer_after(clock, 1)\n    return timer_reschedule(clock, token, 2)\n";
    let diagnostics = crate::typecheck::check(&parse_human(missing).unwrap());
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "undeclared_effect"
                && diagnostic.message.contains("concurrency.timer")
        }),
        "got: {diagnostics:?}"
    );

    let borrowed = "fn borrowed() -> usize effects [concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let token: WakeToken = timer_after(clock, 2)\n    let changed: bool = timer_reschedule(clock, token, 1)\n    let cancelled: bool = timer_cancel(token)\n    return clock_now(clock) + wake_token_signal(token)\n";
    let diagnostics = crate::typecheck::check(&parse_human(borrowed).unwrap());
    assert!(diagnostics.is_empty(), "got: {diagnostics:?}");
}

#[test]
fn interval_timer_issues_fixed_rate_tokens_with_shared_alias_cadence() {
    expect_return(PERIODIC_TIMER_PROGRAM, "fixed_rate", "126");
    let program = parse_human(PERIODIC_TIMER_PROGRAM).unwrap();
    let output = run_entry_json(
        &program,
        "fixed_rate",
        "{ \"args\": {}, \"expect\": { \"return\": 126 } }",
    )
    .unwrap();
    for transition in [
        "\"operation\":\"timer.interval.open\",\"target\":\"0\",\"decision\":\"open\",\"count\":2,\"capacity\":1000000000",
        "\"operation\":\"timer.interval.next\",\"target\":\"0\",\"decision\":\"scheduled\",\"count\":1,\"capacity\":1024",
        "\"operation\":\"clock.advance\",\"target\":\"clock\",\"decision\":\"advanced\",\"count\":2,\"capacity\":1024",
    ] {
        assert!(output.contains(transition), "missing {transition}: {output}");
    }
    let first = output
        .find("\"operation\":\"timer.fire\",\"target\":\"0\"")
        .unwrap();
    let second = output
        .find("\"operation\":\"timer.fire\",\"target\":\"1\"")
        .unwrap();
    let third = output
        .find("\"operation\":\"timer.fire\",\"target\":\"2\"")
        .unwrap();
    assert!(
        first < second && second < third,
        "unstable cadence: {output}"
    );
}

#[test]
fn interval_timer_exposes_missed_ticks_as_ready_one_shot_tokens() {
    expect_return(PERIODIC_TIMER_PROGRAM, "catch_up", "1");
    let program = parse_human(PERIODIC_TIMER_PROGRAM).unwrap();
    let output = run_entry_json(
        &program,
        "catch_up",
        "{ \"args\": {}, \"expect\": { \"return\": 1 } }",
    )
    .unwrap();
    assert_eq!(
        output
            .matches(
                "\"operation\":\"timer.interval.next\",\"target\":\"0\",\"decision\":\"ready\""
            )
            .count(),
        2,
        "{output}"
    );
    assert!(
        output.contains("\"operation\":\"timer.interval.next\",\"target\":\"0\",\"decision\":\"scheduled\",\"count\":1"),
        "{output}"
    );
}

#[test]
fn interval_cancel_is_alias_visible_and_does_not_revoke_issued_ticks() {
    expect_return(PERIODIC_TIMER_PROGRAM, "close_alias", "1");
    let program = parse_human(PERIODIC_TIMER_PROGRAM).unwrap();
    let output = run_entry_json(
        &program,
        "close_alias",
        "{ \"args\": {}, \"expect\": { \"return\": 1 } }",
    )
    .unwrap();
    for transition in [
        "\"operation\":\"timer.interval.cancel\",\"target\":\"0\",\"decision\":\"closed\"",
        "\"operation\":\"timer.interval.cancel\",\"target\":\"0\",\"decision\":\"already_closed\"",
        "\"operation\":\"timer.fire\",\"target\":\"0\",\"decision\":\"signaled\"",
    ] {
        assert!(
            output.contains(transition),
            "missing {transition}: {output}"
        );
    }

    let closed = "fn closed() -> WakeToken effects [concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let interval: IntervalTimer = timer_interval(clock, 1)\n    let cancelled: bool = timer_interval_cancel(interval)\n    return timer_interval_next(interval)\n";
    let program = parse_human(closed).unwrap();
    let error = run_entry_json(&program, "closed", "{ \"args\": {} }").unwrap_err();
    assert!(error.contains("timer_interval_closed"), "got: {error}");
}

#[test]
fn interval_timer_is_bounded_by_clock_range_and_shared_timer_capacity() {
    for (source, expected) in [
        (
            "fn bad() -> IntervalTimer effects [concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    return timer_interval(clock, 0)\n",
            "timer_interval_range",
        ),
        (
            "fn bad() -> IntervalTimer effects [concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let advanced: usize = clock_advance(clock, 1000000000)\n    return timer_interval(clock, 1)\n",
            "clock_range",
        ),
        (
            "fn bad() -> WakeToken effects [concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let advanced: usize = clock_advance(clock, 999999999)\n    let interval: IntervalTimer = timer_interval(clock, 1)\n    let final_tick: WakeToken = timer_interval_next(interval)\n    return timer_interval_next(interval)\n",
            "timer_interval_exhausted",
        ),
    ] {
        let program = parse_human(source).unwrap();
        let error = run_entry_json(&program, "bad", "{ \"args\": {} }").unwrap_err();
        assert!(error.contains(expected), "got: {error}");
    }

    let mut source = String::from(
        "fn overflow() -> WakeToken effects [concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let interval: IntervalTimer = timer_interval(clock, 1)\n",
    );
    for index in 0..=1024 {
        source.push_str(&format!(
            "    let tick{index}: WakeToken = timer_interval_next(interval)\n"
        ));
    }
    source.push_str("    return tick1024\n");
    let program = parse_human(&source).unwrap();
    let error = run_entry_json(&program, "overflow", "{ \"args\": {} }").unwrap_err();
    assert!(error.contains("timer_capacity"), "got: {error}");
}

#[test]
fn interval_timer_is_reserved_typed_effectful_and_copyable() {
    let redeclared =
        "struct IntervalTimer:\n    value: usize\n\nfn demo() -> usize:\n    return 0\n";
    let diagnostics = crate::typecheck::check(&parse_human(redeclared).unwrap());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "reserved_type_name"),
        "got: {diagnostics:?}"
    );

    for source in [
        "fn wrong() -> IntervalTimer effects [concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    return timer_interval(clock, true)\n",
        "fn wrong() -> WakeToken effects [concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    return timer_interval_next(clock)\n",
        "fn wrong() -> bool effects [concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    return timer_interval_cancel(clock)\n",
    ] {
        let diagnostics = crate::typecheck::check(&parse_human(source).unwrap());
        assert!(
            diagnostics.iter().any(|diagnostic| matches!(
                diagnostic.code.as_str(),
                "concurrency_handle_type" | "timer_interval_period_type"
            )),
            "got: {diagnostics:?}"
        );
    }

    let missing = "fn missing() -> IntervalTimer:\n    let clock: AsyncClock = clock_new()\n    return timer_interval(clock, 1)\n";
    let diagnostics = crate::typecheck::check(&parse_human(missing).unwrap());
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "undeclared_effect"
                && diagnostic.message.contains("concurrency.timer")
        }),
        "got: {diagnostics:?}"
    );

    let copyable = "fn copyable() -> usize effects [concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let interval: IntervalTimer = timer_interval(clock, 1)\n    let alias: IntervalTimer = interval\n    let first: WakeToken = timer_interval_next(alias)\n    let second: WakeToken = timer_interval_next(interval)\n    return clock_advance(clock, 2)\n";
    let diagnostics = crate::typecheck::check(&parse_human(copyable).unwrap());
    assert!(diagnostics.is_empty(), "got: {diagnostics:?}");
    expect_return(copyable, "copyable", "2");
}

#[test]
fn logical_clock_and_timer_ranges_are_hard_bounded() {
    for (source, expected) in [
        (
            "fn bad() -> usize effects [concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let token: WakeToken = timer_after(clock, -1)\n    return wake_token_signal(token)\n",
            "timer_range",
        ),
        (
            "fn bad() -> usize effects [concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let advanced: usize = clock_advance(clock, 1000000000)\n    let token: WakeToken = timer_after(clock, 1)\n    return wake_token_signal(token)\n",
            "clock_range",
        ),
        (
            "fn bad() -> bool effects [concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let token: WakeToken = timer_after(clock, 1)\n    return timer_reschedule(clock, token, -1)\n",
            "timer_range",
        ),
        (
            "fn bad() -> bool effects [concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let advanced: usize = clock_advance(clock, 999999999)\n    let token: WakeToken = timer_after(clock, 1)\n    return timer_reschedule(clock, token, 2)\n",
            "clock_range",
        ),
    ] {
        let program = parse_human(source).unwrap();
        let error = run_entry_json(&program, "bad", "{ \"args\": {} }").unwrap_err();
        assert!(error.contains(expected), "got: {error}");
    }
}

#[test]
fn logical_timer_queue_is_hard_bounded() {
    let mut source = String::from(
        "fn overflow() -> usize effects [concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n",
    );
    for index in 0..=1024 {
        source.push_str(&format!(
            "    let token{index}: WakeToken = timer_after(clock, 1)\n"
        ));
    }
    source.push_str("    return 0\n");
    let program = parse_human(&source).unwrap();
    let error = run_entry_json(&program, "overflow", "{ \"args\": {} }").unwrap_err();
    assert!(error.contains("timer_capacity"), "got: {error}");
}

#[test]
fn wake_token_select_chooses_ready_arm_and_cancels_losing_timer() {
    expect_return(SELECT_PROGRAM, "select_timer", "11070");
    expect_return(MANUAL_READY_TIMER_PROGRAM, "manually_ready_timer", "0");
    let program = parse_human(SELECT_PROGRAM).unwrap();
    let output = run_entry_json(
        &program,
        "select_timer",
        "{ \"args\": {}, \"expect\": { \"return\": 11070 } }",
    )
    .unwrap();
    for transition in [
        "\"operation\":\"timer.fire\",\"target\":\"1\",\"decision\":\"signaled\"",
        "\"operation\":\"wake_token.cancel\",\"target\":\"0\",\"decision\":\"select_loser\",\"count\":0,\"capacity\":1024",
        "\"operation\":\"wake_token.select\",\"target\":\"1\",\"decision\":\"selected\",\"count\":1,\"capacity\":1024",
        "\"operation\":\"wake_token.signal\",\"target\":\"slow\",\"decision\":\"cancelled\",\"count\":0,\"capacity\":1024",
        "\"operation\":\"clock.advance\",\"target\":\"clock\",\"decision\":\"advanced\",\"count\":0,\"capacity\":1024",
    ] {
        assert!(output.contains(transition), "missing {transition}: {output}");
    }
}

#[test]
fn pending_select_changes_no_arm_and_can_be_retried_with_a_fresh_set() {
    expect_return(SELECT_PROGRAM, "pending_then_ready", "91");
    let program = parse_human(SELECT_PROGRAM).unwrap();
    let output = run_entry_json(
        &program,
        "pending_then_ready",
        "{ \"args\": {}, \"expect\": { \"return\": 91 } }",
    )
    .unwrap();
    assert!(
        output.contains("\"operation\":\"wake_token.select\",\"target\":\"set\",\"decision\":\"pending\",\"count\":2,\"capacity\":1024"),
        "{output}"
    );
    assert!(
        output.contains("\"operation\":\"wake_token.select\",\"target\":\"1\",\"decision\":\"selected\",\"count\":1,\"capacity\":1024"),
        "{output}"
    );
}

#[test]
fn simultaneous_select_readiness_uses_lowest_input_index() {
    expect_return(SELECT_PROGRAM, "simultaneous_ready", "0");
    let program = parse_human(SELECT_PROGRAM).unwrap();
    let output = run_entry_json(
        &program,
        "simultaneous_ready",
        "{ \"args\": {}, \"expect\": { \"return\": 0 } }",
    )
    .unwrap();
    assert!(
        output.contains("\"operation\":\"wake_token.select\",\"target\":\"1\",\"decision\":\"selected\",\"count\":0,\"capacity\":1024"),
        "{output}"
    );
}

#[test]
fn reactor_wait_advances_to_nearest_timer_and_cancels_losers() {
    expect_return(REACTOR_WAIT_PROGRAM, "wait_for_nearest_timer", "220");
    let program = parse_human(REACTOR_WAIT_PROGRAM).unwrap();
    let output = run_entry_json(
        &program,
        "wait_for_nearest_timer",
        "{ \"args\": {}, \"expect\": { \"return\": 220 } }",
    )
    .unwrap();
    for transition in [
        "\"operation\":\"timer.fire\",\"target\":\"2\",\"decision\":\"signaled\"",
        "\"operation\":\"reactor.wait\",\"target\":\"clock\",\"decision\":\"advanced\",\"count\":2,\"capacity\":1000000000",
        "\"operation\":\"wake_token.cancel\",\"target\":\"0\",\"decision\":\"wait_loser\"",
        "\"operation\":\"wake_token.cancel\",\"target\":\"1\",\"decision\":\"wait_loser\"",
        "\"operation\":\"wake_token.wait\",\"target\":\"2\",\"decision\":\"selected\",\"count\":2,\"capacity\":1024",
        "\"operation\":\"clock.advance\",\"target\":\"clock\",\"decision\":\"advanced\",\"count\":0,\"capacity\":1024",
    ] {
        assert!(output.contains(transition), "missing {transition}: {output}");
    }
}

#[test]
fn reactor_wait_uses_lowest_ready_input_without_advancing_time() {
    expect_return(REACTOR_WAIT_PROGRAM, "ready_without_advance", "0");
    let program = parse_human(REACTOR_WAIT_PROGRAM).unwrap();
    let output = run_entry_json(
        &program,
        "ready_without_advance",
        "{ \"args\": {}, \"expect\": { \"return\": 0 } }",
    )
    .unwrap();
    assert!(
        output.contains("\"operation\":\"reactor.wait\",\"target\":\"clock\",\"decision\":\"ready\",\"count\":0,\"capacity\":1000000000"),
        "{output}"
    );
    assert!(
        output.contains("\"operation\":\"wake_token.wait\",\"target\":\"1\",\"decision\":\"selected\",\"count\":0,\"capacity\":1024"),
        "{output}"
    );
}

#[test]
fn reactor_wait_traps_instead_of_hanging_without_a_future_source() {
    let source = "fn stalled() -> usize effects [alloc, concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let token: WakeToken = wake_token_new()\n    let arms: Vec[WakeToken] = vec_new()\n    push(arms, token)\n    return wake_token_wait(clock, arms)\n";
    let program = parse_human(source).unwrap();
    let error = run_entry_json(&program, "stalled", "{ \"args\": {} }").unwrap_err();
    assert!(error.contains("reactor_stalled"), "got: {error}");
}

#[test]
fn reactor_wait_rejects_timers_from_a_different_clock() {
    let source = "fn mismatched() -> usize effects [alloc, concurrency.task, concurrency.timer]:\n    let first: AsyncClock = clock_new()\n    let second: AsyncClock = clock_new()\n    let token: WakeToken = timer_after(first, 1)\n    let arms: Vec[WakeToken] = vec_new()\n    push(arms, token)\n    return wake_token_wait(second, arms)\n";
    let program = parse_human(source).unwrap();
    let error = run_entry_json(&program, "mismatched", "{ \"args\": {} }").unwrap_err();
    assert!(error.contains("wake_token_wait_clock"), "got: {error}");
}

#[test]
fn reactor_wait_is_typed_effectful_and_consumes_only_its_arm_set() {
    let wrong_type = "fn wrong() -> usize effects [alloc, concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let values: Vec[i32] = vec_new()\n    push(values, 1)\n    return wake_token_wait(clock, values)\n";
    let diagnostics = crate::typecheck::check(&parse_human(wrong_type).unwrap());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "concurrency_handle_type"),
        "got: {diagnostics:?}"
    );

    for missing in ["concurrency.task", "concurrency.timer"] {
        let effects = if missing == "concurrency.task" {
            "alloc, concurrency.timer"
        } else {
            "alloc, concurrency.task"
        };
        let source = format!(
            "fn missing() -> usize effects [{effects}]:\n    let clock: AsyncClock = clock_new()\n    let token: WakeToken = timer_after(clock, 1)\n    let arms: Vec[WakeToken] = vec_new()\n    push(arms, token)\n    return wake_token_wait(clock, arms)\n"
        );
        let diagnostics = crate::typecheck::check(&parse_human(&source).unwrap());
        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "undeclared_effect" && diagnostic.message.contains(missing)
            }),
            "got: {diagnostics:?}"
        );
    }

    let reused = "fn reused() -> usize effects [alloc, concurrency.task, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let token: WakeToken = timer_after(clock, 1)\n    let arms: Vec[WakeToken] = vec_new()\n    push(arms, token)\n    let winner: usize = wake_token_wait(clock, arms)\n    return winner + arms.len + clock_now(clock)\n";
    let diagnostics = crate::typecheck::check(&parse_human(reused).unwrap());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "use_after_move"),
        "got: {diagnostics:?}"
    );
    assert!(
        !diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "use_after_move" && diagnostic.message.contains("clock")
        }),
        "clock must remain readable: {diagnostics:?}"
    );
}

#[test]
fn heterogeneous_wait_advances_a_task_timer_and_preserves_resource_handles() {
    expect_return(
        HETEROGENEOUS_READINESS_PROGRAM,
        "timer_wakes_task_before_timeout",
        "143",
    );
    let program = parse_human(HETEROGENEOUS_READINESS_PROGRAM).unwrap();
    let output = run_entry_json(
        &program,
        "timer_wakes_task_before_timeout",
        "{ \"args\": {}, \"expect\": { \"return\": 143 } }",
    )
    .unwrap();
    for transition in [
        "\"operation\":\"readiness.arm\",\"target\":\"channel:channel\",\"decision\":\"pending\"",
        "\"operation\":\"readiness.arm\",\"target\":\"task:task\",\"decision\":\"pending\"",
        "\"operation\":\"timer.fire\",\"target\":\"0\",\"decision\":\"signaled\"",
        "\"operation\":\"reactor.wait\",\"target\":\"clock\",\"decision\":\"advanced\",\"count\":2",
        "\"operation\":\"wake_token.cancel\",\"target\":\"1\",\"decision\":\"readiness_loser\"",
        "\"operation\":\"readiness.wait\",\"target\":\"task:task\",\"decision\":\"selected\",\"count\":1",
        "\"operation\":\"task.await\",\"target\":\"task\",\"decision\":\"completed\"",
        "\"operation\":\"channel.recv\",\"target\":\"channel\",\"decision\":\"received\"",
    ] {
        assert!(output.contains(transition), "missing {transition}: {output}");
    }
}

#[test]
fn heterogeneous_wait_uses_lowest_ready_kind_without_consuming_losers() {
    expect_return(
        HETEROGENEOUS_READINESS_PROGRAM,
        "lowest_ready_kind_wins",
        "68",
    );
    let program = parse_human(HETEROGENEOUS_READINESS_PROGRAM).unwrap();
    let output = run_entry_json(
        &program,
        "lowest_ready_kind_wins",
        "{ \"args\": {}, \"expect\": { \"return\": 68 } }",
    )
    .unwrap();
    assert!(
        output.contains("\"operation\":\"reactor.wait\",\"target\":\"clock\",\"decision\":\"ready\",\"count\":0"),
        "{output}"
    );
    assert!(
        output.contains("\"operation\":\"readiness.wait\",\"target\":\"task:task\",\"decision\":\"selected\",\"count\":0"),
        "{output}"
    );
}

#[test]
fn heterogeneous_wait_treats_closed_channel_as_immediately_readable() {
    expect_return(
        HETEROGENEOUS_READINESS_PROGRAM,
        "closed_channel_is_readable",
        "7",
    );
}

#[test]
fn heterogeneous_arms_observe_handle_changes_after_adapter_creation() {
    let source = "fn identity(value: i32) -> i32:\n    return value\n\nfn changed() -> i32 effects [alloc, concurrency.task, concurrency.channel, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let task: Task[i32] = task_spawn_parked(identity, 6)\n    let channel: Channel[i32] = channel_new(1)\n    let channel_arm: ReadinessArm = readiness_arm_channel(channel)\n    let sent: bool = channel_send(channel, 4)\n    let channel_arms: Vec[ReadinessArm] = vec_new()\n    push(channel_arms, channel_arm)\n    let channel_winner: usize = readiness_wait(clock, channel_arms)\n    let received: i32 = unwrap_or(channel_recv(channel), 0)\n    let task_arm: ReadinessArm = readiness_arm_task(task)\n    let woke: bool = task_wake(task)\n    let task_arms: Vec[ReadinessArm] = vec_new()\n    push(task_arms, task_arm)\n    let task_winner: usize = readiness_wait(clock, task_arms)\n    if !sent || !woke:\n        return -1\n    return i32(channel_winner) * 1000 + received * 100 + i32(task_winner) * 10 + task_await(task)\n";
    expect_return(source, "changed", "406");
}

#[test]
fn heterogeneous_wait_traps_when_task_and_channel_cannot_progress() {
    let source = "fn identity(value: i32) -> i32:\n    return value\n\nfn stalled() -> usize effects [alloc, concurrency.task, concurrency.channel, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let task: Task[i32] = task_spawn_parked(identity, 1)\n    let channel: Channel[i32] = channel_new(1)\n    let arms: Vec[ReadinessArm] = vec_new()\n    push(arms, readiness_arm_task(task))\n    push(arms, readiness_arm_channel(channel))\n    return readiness_wait(clock, arms)\n";
    let program = parse_human(source).unwrap();
    let error = run_entry_json(&program, "stalled", "{ \"args\": {} }").unwrap_err();
    assert!(error.contains("reactor_stalled"), "got: {error}");
}

#[test]
fn heterogeneous_wait_rejects_cross_clock_task_sources_and_duplicates() {
    let cross_clock = "fn identity(value: i32) -> i32:\n    return value\n\nfn mismatched() -> usize effects [alloc, concurrency.task, concurrency.channel, concurrency.timer]:\n    let first: AsyncClock = clock_new()\n    let second: AsyncClock = clock_new()\n    let timer: WakeToken = timer_after(first, 1)\n    let task: Task[i32] = task_spawn_on(timer, identity, 1)\n    let arms: Vec[ReadinessArm] = vec_new()\n    push(arms, readiness_arm_task(task))\n    return readiness_wait(second, arms)\n";
    let program = parse_human(cross_clock).unwrap();
    let error = run_entry_json(&program, "mismatched", "{ \"args\": {} }").unwrap_err();
    assert!(error.contains("readiness_wait_clock"), "got: {error}");

    let duplicate = "fn identity(value: i32) -> i32:\n    return value\n\nfn duplicate() -> usize effects [alloc, concurrency.task, concurrency.channel, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let task: Task[i32] = task_spawn(identity, 1)\n    let arms: Vec[ReadinessArm] = vec_new()\n    push(arms, readiness_arm_task(task))\n    push(arms, readiness_arm_task(task))\n    return readiness_wait(clock, arms)\n";
    let program = parse_human(duplicate).unwrap();
    let error = run_entry_json(&program, "duplicate", "{ \"args\": {} }").unwrap_err();
    assert!(error.contains("readiness_wait_duplicate"), "got: {error}");
}

#[test]
fn heterogeneous_wait_rejects_over_capacity_and_token_arms_with_task_waiters() {
    let mut overflow = String::from(
        "fn overflow() -> usize effects [alloc, concurrency.task, concurrency.channel, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let arms: Vec[ReadinessArm] = vec_new()\n",
    );
    for index in 0..=1024 {
        overflow.push_str(&format!(
            "    let token{index}: WakeToken = wake_token_new()\n    push(arms, readiness_arm_token(token{index}))\n"
        ));
    }
    overflow.push_str("    return readiness_wait(clock, arms)\n");
    let program = parse_human(&overflow).unwrap();
    let error = run_entry_json(&program, "overflow", "{ \"args\": {} }").unwrap_err();
    assert!(error.contains("readiness_wait_capacity"), "got: {error}");

    let registered = "fn identity(value: i32) -> i32:\n    return value\n\nfn registered() -> ReadinessArm effects [concurrency.task]:\n    let token: WakeToken = wake_token_new()\n    let task: Task[i32] = task_spawn_on(token, identity, 1)\n    return readiness_arm_token(token)\n";
    let program = parse_human(registered).unwrap();
    let error = run_entry_json(&program, "registered", "{ \"args\": {} }").unwrap_err();
    assert!(error.contains("readiness_arm_waiters"), "got: {error}");
}

#[test]
fn heterogeneous_readiness_is_typed_effectful_and_consumes_only_arm_sets() {
    let wrong_type = "fn wrong() -> usize effects [alloc, concurrency.task, concurrency.channel, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let tokens: Vec[WakeToken] = vec_new()\n    return readiness_wait(clock, tokens)\n";
    let diagnostics = crate::typecheck::check(&parse_human(wrong_type).unwrap());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "concurrency_handle_type"),
        "got: {diagnostics:?}"
    );

    for missing in [
        "concurrency.task",
        "concurrency.channel",
        "concurrency.timer",
    ] {
        let effects = [
            "alloc",
            "concurrency.task",
            "concurrency.channel",
            "concurrency.timer",
        ]
        .into_iter()
        .filter(|effect| *effect != missing)
        .collect::<Vec<_>>()
        .join(", ");
        let source = format!(
            "fn identity(value: i32) -> i32:\n    return value\n\nfn missing() -> usize effects [{effects}]:\n    let clock: AsyncClock = clock_new()\n    let task: Task[i32] = task_spawn(identity, 1)\n    let channel: Channel[i32] = channel_new(1)\n    let arms: Vec[ReadinessArm] = vec_new()\n    push(arms, readiness_arm_task(task))\n    push(arms, readiness_arm_channel(channel))\n    return readiness_wait(clock, arms)\n"
        );
        let diagnostics = crate::typecheck::check(&parse_human(&source).unwrap());
        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "undeclared_effect" && diagnostic.message.contains(missing)
            }),
            "missing {missing}: {diagnostics:?}"
        );
    }

    let reused = "fn identity(value: i32) -> i32:\n    return value\n\nfn reused() -> i32 effects [alloc, concurrency.task, concurrency.channel, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let task: Task[i32] = task_spawn(identity, 4)\n    let channel: Channel[i32] = channel_new(1)\n    let arms: Vec[ReadinessArm] = vec_new()\n    push(arms, readiness_arm_task(task))\n    let winner: usize = readiness_wait(clock, arms)\n    let sent: bool = channel_send(channel, 3)\n    return task_await(task) + clock_now(clock) + winner + arms.len\n";
    let diagnostics = crate::typecheck::check(&parse_human(reused).unwrap());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "use_after_move"),
        "got: {diagnostics:?}"
    );
    for binding in ["task", "clock", "channel"] {
        assert!(
            !diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "use_after_move" && diagnostic.message.contains(binding)
            }),
            "{binding} must remain available: {diagnostics:?}"
        );
    }
}

#[test]
fn readiness_timeout_selects_arms_or_preserves_sources_at_the_deadline() {
    expect_return(READINESS_TIMEOUT_PROGRAM, "timer_task_wins", "242");
    expect_return(READINESS_TIMEOUT_PROGRAM, "equal_token_deadline_wins", "3");
    expect_return(
        READINESS_TIMEOUT_PROGRAM,
        "timeout_preserves_handles",
        "9489",
    );
    expect_return(READINESS_TIMEOUT_PROGRAM, "empty_sleep_fires_timer", "730");
    expect_return(
        READINESS_TIMEOUT_PROGRAM,
        "ready_task_cancels_losing_timer",
        "60",
    );
}

#[test]
fn readiness_timeout_receipts_distinguish_selection_from_timeout() {
    let program = parse_human(READINESS_TIMEOUT_PROGRAM).unwrap();
    let selected = run_entry_json(
        &program,
        "timer_task_wins",
        "{ \"args\": {}, \"expect\": { \"return\": 242 } }",
    )
    .unwrap();
    for transition in [
        "\"operation\":\"timer.fire\",\"target\":\"0\",\"decision\":\"signaled\"",
        "\"operation\":\"reactor.wait\",\"target\":\"clock\",\"decision\":\"advanced\",\"count\":2",
        "\"operation\":\"readiness.wait_timeout\",\"target\":\"task:task\",\"decision\":\"selected\",\"count\":1",
    ] {
        assert!(
            selected.contains(transition),
            "missing {transition}: {selected}"
        );
    }

    let timed_out = run_entry_json(
        &program,
        "timeout_preserves_handles",
        "{ \"args\": {}, \"expect\": { \"return\": 9489 } }",
    )
    .unwrap();
    for transition in [
        "\"operation\":\"reactor.wait\",\"target\":\"clock\",\"decision\":\"advanced\",\"count\":4",
        "\"operation\":\"readiness.wait_timeout\",\"target\":\"clock\",\"decision\":\"timed_out\",\"count\":4",
    ] {
        assert!(
            timed_out.contains(transition),
            "missing {transition}: {timed_out}"
        );
    }
    assert!(
        !timed_out.contains("\"operation\":\"wake_token.cancel\""),
        "timeout must preserve every observed source: {timed_out}"
    );
}

#[test]
fn readiness_timeout_rejects_cross_clock_and_out_of_range_deadlines() {
    let cross_clock = "fn mismatched() -> Option[usize] effects [alloc, concurrency.task, concurrency.channel, concurrency.timer]:\n    let first: AsyncClock = clock_new()\n    let second: AsyncClock = clock_new()\n    let token: WakeToken = timer_after(first, 1)\n    let arms: Vec[ReadinessArm] = vec_new()\n    push(arms, readiness_arm_token(token))\n    return readiness_wait_timeout(second, arms, 0)\n";
    let out_of_range = "fn bad() -> Option[usize] effects [alloc, concurrency.task, concurrency.channel, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let arms: Vec[ReadinessArm] = vec_new()\n    return readiness_wait_timeout(clock, arms, 1000000001)\n";
    let clock_overflow = "fn bad() -> Option[usize] effects [alloc, concurrency.task, concurrency.channel, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let advanced: usize = clock_advance(clock, 1000000000)\n    let arms: Vec[ReadinessArm] = vec_new()\n    return readiness_wait_timeout(clock, arms, 1)\n";
    for (source, entry, expected) in [
        (cross_clock, "mismatched", "readiness_wait_clock"),
        (out_of_range, "bad", "readiness_timeout_range"),
        (clock_overflow, "bad", "clock_range"),
    ] {
        let program = parse_human(source).unwrap();
        let error = run_entry_json(&program, entry, "{ \"args\": {} }").unwrap_err();
        assert!(error.contains(expected), "got: {error}");
    }
}

#[test]
fn readiness_timeout_is_typed_effectful_and_moves_only_the_arm_set() {
    let wrong_arms = "fn wrong() -> Option[usize] effects [alloc, concurrency.task, concurrency.channel, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let values: Vec[i32] = vec_new()\n    return readiness_wait_timeout(clock, values, 1)\n";
    let wrong_delay = "fn wrong() -> Option[usize] effects [alloc, concurrency.task, concurrency.channel, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let arms: Vec[ReadinessArm] = vec_new()\n    return readiness_wait_timeout(clock, arms, \"later\")\n";
    for (source, expected) in [
        (wrong_arms, "concurrency_handle_type"),
        (wrong_delay, "clock_tick_type"),
    ] {
        let diagnostics = crate::typecheck::check(&parse_human(source).unwrap());
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == expected),
            "got: {diagnostics:?}"
        );
    }

    for missing in [
        "concurrency.task",
        "concurrency.channel",
        "concurrency.timer",
    ] {
        let effects = [
            "alloc",
            "concurrency.task",
            "concurrency.channel",
            "concurrency.timer",
        ]
        .into_iter()
        .filter(|effect| *effect != missing)
        .collect::<Vec<_>>()
        .join(", ");
        let source = format!(
            "fn missing() -> Option[usize] effects [{effects}]:\n    let clock: AsyncClock = clock_new()\n    let arms: Vec[ReadinessArm] = vec_new()\n    return readiness_wait_timeout(clock, arms, 1)\n"
        );
        let diagnostics = crate::typecheck::check(&parse_human(&source).unwrap());
        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "undeclared_effect" && diagnostic.message.contains(missing)
            }),
            "missing {missing}: {diagnostics:?}"
        );
    }

    let reused = "fn identity(value: i32) -> i32:\n    return value\n\nfn reused() -> i32 effects [alloc, concurrency.task, concurrency.channel, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let task: Task[i32] = task_spawn(identity, 4)\n    let channel: Channel[i32] = channel_new(1)\n    let arms: Vec[ReadinessArm] = vec_new()\n    push(arms, readiness_arm_task(task))\n    let winner: usize = unwrap_or(readiness_wait_timeout(clock, arms, 1), 0)\n    let sent: bool = channel_send(channel, 3)\n    return task_await(task) + i32(clock_now(clock)) + i32(winner) + i32(arms.len)\n";
    let diagnostics = crate::typecheck::check(&parse_human(reused).unwrap());
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "use_after_move" && diagnostic.message.contains("arms")
        }),
        "got: {diagnostics:?}"
    );
    for binding in ["task", "clock", "channel"] {
        assert!(
            !diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "use_after_move" && diagnostic.message.contains(binding)
            }),
            "{binding} must remain available: {diagnostics:?}"
        );
    }
}

#[test]
fn writable_channel_readiness_selects_open_capacity_and_distinguishes_reads() {
    expect_return(WRITABLE_READINESS_PROGRAM, "initially_writable", "0");
    expect_return(
        WRITABLE_READINESS_PROGRAM,
        "read_and_write_are_distinct",
        "10",
    );
}

#[test]
fn writable_channel_arms_track_capacity_and_close_after_adapter_creation() {
    expect_return(WRITABLE_READINESS_PROGRAM, "tracks_capacity_changes", "927");
    expect_return(WRITABLE_READINESS_PROGRAM, "close_disables_writes", "81");

    let duplicate = "fn duplicate() -> usize effects [alloc, concurrency.task, concurrency.channel, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let channel: Channel[i32] = channel_new(1)\n    let arms: Vec[ReadinessArm] = vec_new()\n    push(arms, readiness_arm_channel_send(channel))\n    push(arms, readiness_arm_channel_send(channel))\n    return readiness_wait(clock, arms)\n";
    let program = parse_human(duplicate).unwrap();
    let error = run_entry_json(&program, "duplicate", "{ \"args\": {} }").unwrap_err();
    assert!(error.contains("readiness_wait_duplicate"), "got: {error}");
}

#[test]
fn writable_channel_readiness_emits_source_specific_receipts() {
    let program = parse_human(WRITABLE_READINESS_PROGRAM).unwrap();
    let selected = run_entry_json(
        &program,
        "initially_writable",
        "{ \"args\": {}, \"expect\": { \"return\": 0 } }",
    )
    .unwrap();
    for transition in [
        "\"operation\":\"readiness.arm\",\"target\":\"channel_send:channel\",\"decision\":\"ready\"",
        "\"operation\":\"readiness.wait\",\"target\":\"channel_send:channel\",\"decision\":\"selected\",\"count\":0",
    ] {
        assert!(
            selected.contains(transition),
            "missing {transition}: {selected}"
        );
    }

    let transitioned = run_entry_json(
        &program,
        "tracks_capacity_changes",
        "{ \"args\": {}, \"expect\": { \"return\": 927 } }",
    )
    .unwrap();
    for transition in [
        "\"operation\":\"readiness.wait_timeout\",\"target\":\"clock\",\"decision\":\"timed_out\",\"count\":2",
        "\"operation\":\"readiness.wait\",\"target\":\"channel_send:channel\",\"decision\":\"selected\",\"count\":0",
    ] {
        assert!(
            transitioned.contains(transition),
            "missing {transition}: {transitioned}"
        );
    }
}

#[test]
fn writable_channel_readiness_is_typed_effectful_and_borrowed() {
    let wrong_type = "fn wrong() -> ReadinessArm effects [concurrency.channel]:\n    return readiness_arm_channel_send(1)\n";
    let diagnostics = crate::typecheck::check(&parse_human(wrong_type).unwrap());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "concurrency_handle_type"),
        "got: {diagnostics:?}"
    );

    let missing_effect = "fn missing() -> ReadinessArm:\n    let channel: Channel[i32] = channel_new(1)\n    return readiness_arm_channel_send(channel)\n";
    let diagnostics = crate::typecheck::check(&parse_human(missing_effect).unwrap());
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "undeclared_effect"
                && diagnostic.message.contains("concurrency.channel")
        }),
        "got: {diagnostics:?}"
    );

    let borrowed = "fn borrowed() -> i32 effects [alloc, concurrency.task, concurrency.channel, concurrency.timer]:\n    let clock: AsyncClock = clock_new()\n    let channel: Channel[i32] = channel_new(1)\n    let arms: Vec[ReadinessArm] = vec_new()\n    push(arms, readiness_arm_channel_send(channel))\n    let winner: usize = readiness_wait(clock, arms)\n    let sent: bool = channel_send(channel, 3)\n    if !sent:\n        return -1\n    return i32(winner) + i32(arms.len)\n";
    let diagnostics = crate::typecheck::check(&parse_human(borrowed).unwrap());
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "use_after_move" && diagnostic.message.contains("arms")
        }),
        "got: {diagnostics:?}"
    );
    assert!(
        !diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "use_after_move" && diagnostic.message.contains("channel")
        }),
        "channel must remain available: {diagnostics:?}"
    );
}

#[test]
fn file_readiness_selects_existing_files_without_consuming_read_budget() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_file_readiness_existing_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("ready.txt");
    std::fs::write(&path, "ready").unwrap();
    let path = path.display().to_string().replace('\\', "\\\\");
    let input =
        format!("{{ \"args\": {{ \"path\": \"{path}\" }}, \"expect\": {{ \"return\": 0 }} }}");
    let program = parse_human(FILE_READINESS_PROGRAM).unwrap();
    let output = run_entry_json_hosted(
        &program,
        "existing",
        &input,
        None,
        vec!["fs.read".to_string()],
        Vec::new(),
    )
    .unwrap();
    for transition in [
        "\"op\": \"readiness_read\"",
        "\"bytes\": 0",
        "\"operation\":\"readiness.arm\",\"target\":\"file_read:path\",\"decision\":\"ready\"",
        "\"operation\":\"readiness.wait\",\"target\":\"file_read:path\",\"decision\":\"selected\",\"count\":0",
    ] {
        assert!(output.contains(transition), "missing {transition}: {output}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn file_readiness_times_out_or_tracks_creation_after_adapter_construction() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_file_readiness_transition_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("later.txt");
    let path_arg = path.display().to_string().replace('\\', "\\\\");
    let program = parse_human(FILE_READINESS_PROGRAM).unwrap();

    let missing_input =
        format!("{{ \"args\": {{ \"path\": \"{path_arg}\" }}, \"expect\": {{ \"return\": 92 }} }}");
    let missing = run_entry_json_hosted(
        &program,
        "missing",
        &missing_input,
        None,
        vec!["fs.read".to_string()],
        Vec::new(),
    )
    .unwrap();
    assert!(
        missing.contains("\"operation\":\"readiness.wait_timeout\",\"target\":\"clock\",\"decision\":\"timed_out\",\"count\":2"),
        "{missing}"
    );

    let appears_input =
        format!("{{ \"args\": {{ \"path\": \"{path_arg}\" }}, \"expect\": {{ \"return\": 50 }} }}");
    let appears = run_entry_json_hosted(
        &program,
        "appears",
        &appears_input,
        None,
        vec!["fs.read".to_string(), "fs.write".to_string()],
        Vec::new(),
    )
    .unwrap();
    for transition in [
        "\"operation\":\"readiness.arm\",\"target\":\"file_read:path\",\"decision\":\"pending\"",
        "\"operation\":\"readiness.wait\",\"target\":\"file_read:path\",\"decision\":\"selected\",\"count\":0",
    ] {
        assert!(
            appears.contains(transition),
            "missing {transition}: {appears}"
        );
    }
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "ready");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn file_readiness_requires_a_host_and_rejects_duplicate_resolved_paths() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_file_readiness_duplicate_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("ready.txt");
    std::fs::write(&path, "ready").unwrap();
    let path_arg = path.display().to_string().replace('\\', "\\\\");

    let program = parse_human(FILE_READINESS_PROGRAM).unwrap();
    let input = format!("{{ \"args\": {{ \"path\": \"{path_arg}\" }} }}");
    let error = run_entry_json(&program, "existing", &input).unwrap_err();
    assert!(error.contains("io_unavailable"), "got: {error}");
    let error = run_entry_json_hosted(&program, "existing", &input, None, Vec::new(), Vec::new())
        .unwrap_err();
    assert!(error.contains("io_denied"), "got: {error}");

    let duplicate = "fn duplicate(first: Path, second: Path) -> usize effects [alloc, concurrency.task, concurrency.channel, concurrency.timer, fs.read]:\n    let clock: AsyncClock = clock_new()\n    let arms: Vec[ReadinessArm] = vec_new()\n    push(arms, readiness_arm_file_read(first))\n    push(arms, readiness_arm_file_read(second))\n    return readiness_wait(clock, arms)\n";
    let program = parse_human(duplicate).unwrap();
    let input =
        format!("{{ \"args\": {{ \"first\": \"{path_arg}\", \"second\": \"{path_arg}\" }} }}");
    let error = run_entry_json_hosted(
        &program,
        "duplicate",
        &input,
        None,
        vec!["fs.read".to_string()],
        Vec::new(),
    )
    .unwrap_err();
    assert!(error.contains("readiness_wait_duplicate"), "got: {error}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn file_readiness_is_path_typed_fs_effectful_and_borrowed() {
    let wrong_type = "fn wrong(path: str) -> ReadinessArm effects [fs.read]:\n    return readiness_arm_file_read(path)\n";
    let diagnostics = crate::typecheck::check(&parse_human(wrong_type).unwrap());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "concurrency_handle_type"),
        "got: {diagnostics:?}"
    );

    let missing_effect =
        "fn missing(path: Path) -> ReadinessArm:\n    return readiness_arm_file_read(path)\n";
    let diagnostics = crate::typecheck::check(&parse_human(missing_effect).unwrap());
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "undeclared_effect" && diagnostic.message.contains("fs.read")
        }),
        "got: {diagnostics:?}"
    );

    let borrowed = "fn borrowed(path: Path) -> i32 effects [alloc, concurrency.task, concurrency.channel, concurrency.timer, fs.read]:\n    let clock: AsyncClock = clock_new()\n    let arms: Vec[ReadinessArm] = vec_new()\n    push(arms, readiness_arm_file_read(path))\n    let winner: usize = unwrap_or(readiness_wait_timeout(clock, arms, 0), 0)\n    return i32(winner) + i32(str_len(core_path_to_str(path))) + i32(arms.len)\n";
    let diagnostics = crate::typecheck::check(&parse_human(borrowed).unwrap());
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "use_after_move" && diagnostic.message.contains("arms")
        }),
        "got: {diagnostics:?}"
    );
    assert!(
        !diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "use_after_move" && diagnostic.message.contains("path")
        }),
        "path must remain available: {diagnostics:?}"
    );
}

#[test]
fn file_create_readiness_selects_an_available_no_clobber_destination() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_file_create_available_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("available.txt");
    let path_arg = path.display().to_string().replace('\\', "\\\\");
    let input =
        format!("{{ \"args\": {{ \"path\": \"{path_arg}\" }}, \"expect\": {{ \"return\": 0 }} }}");
    let program = parse_human(FILE_CREATE_READINESS_PROGRAM).unwrap();
    let output = run_entry_json_hosted(
        &program,
        "available",
        &input,
        None,
        vec!["fs.write".to_string()],
        Vec::new(),
    )
    .unwrap();
    for transition in [
        "\"op\": \"readiness_create\"",
        "\"bytes\": 0",
        "\"operation\":\"readiness.arm\",\"target\":\"file_create:path\",\"decision\":\"ready\"",
        "\"operation\":\"readiness.wait\",\"target\":\"file_create:path\",\"decision\":\"selected\",\"count\":0",
    ] {
        assert!(output.contains(transition), "missing {transition}: {output}");
    }
    assert!(
        !path.exists(),
        "readiness observation must not create the file"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn file_create_readiness_tracks_existing_and_newly_claimed_destinations() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_file_create_transition_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let existing_path = dir.join("existing.txt");
    std::fs::write(&existing_path, "old").unwrap();
    let existing_arg = existing_path.display().to_string().replace('\\', "\\\\");
    let program = parse_human(FILE_CREATE_READINESS_PROGRAM).unwrap();
    let input = format!(
        "{{ \"args\": {{ \"path\": \"{existing_arg}\" }}, \"expect\": {{ \"return\": 81 }} }}"
    );
    let existing = run_entry_json_hosted(
        &program,
        "existing",
        &input,
        None,
        vec!["fs.write".to_string()],
        Vec::new(),
    )
    .unwrap();
    assert!(
        existing.contains("\"operation\":\"readiness.wait_timeout\",\"target\":\"clock\",\"decision\":\"timed_out\",\"count\":1"),
        "{existing}"
    );

    let claimed_path = dir.join("claimed.txt");
    let claimed_arg = claimed_path.display().to_string().replace('\\', "\\\\");
    let input = format!(
        "{{ \"args\": {{ \"path\": \"{claimed_arg}\" }}, \"expect\": {{ \"return\": 592 }} }}"
    );
    let claimed = run_entry_json_hosted(
        &program,
        "claimed",
        &input,
        None,
        vec!["fs.write".to_string()],
        Vec::new(),
    )
    .unwrap();
    for transition in [
        "\"operation\":\"readiness.arm\",\"target\":\"file_create:path\",\"decision\":\"ready\"",
        "\"operation\":\"readiness.wait_timeout\",\"target\":\"clock\",\"decision\":\"timed_out\",\"count\":2",
    ] {
        assert!(
            claimed.contains(transition),
            "missing {transition}: {claimed}"
        );
    }
    assert_eq!(std::fs::read_to_string(&claimed_path).unwrap(), "ready");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn file_create_readiness_requires_a_host_and_rejects_duplicate_destinations() {
    let dir = std::env::temp_dir().join(format!(
        "forgentex_file_create_duplicate_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("available.txt");
    let path_arg = path.display().to_string().replace('\\', "\\\\");
    let program = parse_human(FILE_CREATE_READINESS_PROGRAM).unwrap();
    let input = format!("{{ \"args\": {{ \"path\": \"{path_arg}\" }} }}");
    let error = run_entry_json(&program, "available", &input).unwrap_err();
    assert!(error.contains("io_unavailable"), "got: {error}");
    let error = run_entry_json_hosted(&program, "available", &input, None, Vec::new(), Vec::new())
        .unwrap_err();
    assert!(error.contains("io_denied"), "got: {error}");

    let duplicate = "fn duplicate(first: Path, second: Path) -> usize effects [alloc, concurrency.task, concurrency.channel, concurrency.timer, fs.write]:\n    let clock: AsyncClock = clock_new()\n    let arms: Vec[ReadinessArm] = vec_new()\n    push(arms, readiness_arm_file_create(first))\n    push(arms, readiness_arm_file_create(second))\n    return readiness_wait(clock, arms)\n";
    let program = parse_human(duplicate).unwrap();
    let input =
        format!("{{ \"args\": {{ \"first\": \"{path_arg}\", \"second\": \"{path_arg}\" }} }}");
    let error = run_entry_json_hosted(
        &program,
        "duplicate",
        &input,
        None,
        vec!["fs.write".to_string()],
        Vec::new(),
    )
    .unwrap_err();
    assert!(error.contains("readiness_wait_duplicate"), "got: {error}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn file_create_readiness_is_path_typed_fs_effectful_and_borrowed() {
    let wrong_type = "fn wrong(path: str) -> ReadinessArm effects [fs.write]:\n    return readiness_arm_file_create(path)\n";
    let diagnostics = crate::typecheck::check(&parse_human(wrong_type).unwrap());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "concurrency_handle_type"),
        "got: {diagnostics:?}"
    );

    let missing_effect =
        "fn missing(path: Path) -> ReadinessArm:\n    return readiness_arm_file_create(path)\n";
    let diagnostics = crate::typecheck::check(&parse_human(missing_effect).unwrap());
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "undeclared_effect" && diagnostic.message.contains("fs.write")
        }),
        "got: {diagnostics:?}"
    );

    let borrowed = "fn borrowed(path: Path) -> i32 effects [alloc, concurrency.task, concurrency.channel, concurrency.timer, fs.write]:\n    let clock: AsyncClock = clock_new()\n    let arms: Vec[ReadinessArm] = vec_new()\n    push(arms, readiness_arm_file_create(path))\n    let winner: usize = unwrap_or(readiness_wait_timeout(clock, arms, 0), 0)\n    return i32(winner) + i32(str_len(core_path_to_str(path))) + i32(arms.len)\n";
    let diagnostics = crate::typecheck::check(&parse_human(borrowed).unwrap());
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "use_after_move" && diagnostic.message.contains("arms")
        }),
        "got: {diagnostics:?}"
    );
    assert!(
        !diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "use_after_move" && diagnostic.message.contains("path")
        }),
        "path must remain available: {diagnostics:?}"
    );
}

#[test]
fn wake_token_select_rejects_over_capacity_duplicates_and_registered_waiters() {
    let mut overflow = String::from(
        "fn overflow() -> usize effects [alloc, concurrency.task]:\n    let arms: Vec[WakeToken] = vec_new()\n",
    );
    for index in 0..=1024 {
        overflow.push_str(&format!(
            "    let token{index}: WakeToken = wake_token_new()\n    push(arms, token{index})\n"
        ));
    }
    overflow.push_str("    return unwrap_or(wake_token_select(arms), 0)\n");
    for (source, expected) in [
        (overflow, "wake_token_select_capacity"),
        (
            "fn duplicate() -> usize effects [alloc, concurrency.task]:\n    let token: WakeToken = wake_token_new()\n    let arms: Vec[WakeToken] = vec_new()\n    push(arms, token)\n    push(arms, token)\n    return unwrap_or(wake_token_select(arms), 0)\n".to_string(),
            "wake_token_select_duplicate",
        ),
        (
            "fn identity(x: usize) -> usize:\n    return x\n\nfn registered() -> usize effects [alloc, concurrency.task]:\n    let token: WakeToken = wake_token_new()\n    let task: Task[usize] = task_spawn_on(token, identity, 1)\n    let arms: Vec[WakeToken] = vec_new()\n    push(arms, token)\n    return unwrap_or(wake_token_select(arms), 0)\n".to_string(),
            "wake_token_select_waiters",
        ),
    ] {
        let program = parse_human(&source).unwrap();
        let entry = if source.contains("fn overflow") {
            "overflow"
        } else if source.contains("fn duplicate") {
            "duplicate"
        } else {
            "registered"
        };
        let error = run_entry_json(&program, entry, "{ \"args\": {} }").unwrap_err();
        assert!(error.contains(expected), "got: {error}");
    }
}

#[test]
fn wake_token_select_is_typed_effectful_and_consumes_its_vector() {
    let wrong_type = "fn wrong() -> usize effects [alloc, concurrency.task]:\n    let values: Vec[i32] = vec_new()\n    push(values, 1)\n    return unwrap_or(wake_token_select(values), 0)\n";
    let diagnostics = crate::typecheck::check(&parse_human(wrong_type).unwrap());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "concurrency_handle_type"),
        "got: {diagnostics:?}"
    );

    let missing_effect = "fn missing() -> usize effects [alloc]:\n    let token: WakeToken = wake_token_new()\n    let arms: Vec[WakeToken] = vec_new()\n    push(arms, token)\n    return unwrap_or(wake_token_select(arms), 0)\n";
    let diagnostics = crate::typecheck::check(&parse_human(missing_effect).unwrap());
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "undeclared_effect"
                && diagnostic.message.contains("concurrency.task")
        }),
        "got: {diagnostics:?}"
    );

    let reused = "fn reused() -> usize effects [alloc, concurrency.task]:\n    let token: WakeToken = wake_token_new()\n    let arms: Vec[WakeToken] = vec_new()\n    push(arms, token)\n    let winner: usize = unwrap_or(wake_token_select(arms), 0)\n    return winner + arms.len\n";
    let diagnostics = crate::typecheck::check(&parse_human(reused).unwrap());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "use_after_move"),
        "got: {diagnostics:?}"
    );
}

#[test]
fn task_group_awaits_children_in_deterministic_spawn_order() {
    expect_return(TASK_GROUP_PROGRAM, "ordered_group", "123");
}

#[test]
fn task_group_full_join_drains_owned_children_from_the_consumed_scope() {
    let program = parse_human(TASK_GROUP_PROGRAM).unwrap();
    let output = run_entry_json(
        &program,
        "ordered_group",
        "{ \"args\": {}, \"expect\": { \"return\": 123 } }",
    )
    .unwrap();
    assert!(
        output.contains(
            "\"group\": {\"$task_group\":{\"state\":\"consumed\",\"capacity\":3,\"children\":0}}"
        ),
        "{output}"
    );
}

#[test]
fn task_group_await_next_advances_one_child_and_releases_capacity() {
    let source = "fn identity(x: i32) -> i32:\n    return x\n\nfn incremental_group() -> i32 effects [concurrency.task]:\n    let group: TaskGroup[i32] = task_group_new(2)\n    let first: bool = task_group_spawn(group, identity, 1)\n    let second: bool = task_group_spawn(group, identity, 2)\n    let blocked: bool = task_group_spawn(group, identity, 99)\n    let next: i32 = unwrap_or(task_group_await_next(group), 0)\n    let third: bool = task_group_spawn(group, identity, 3)\n    let rest: Vec[i32] = task_group_await_all(group)\n    if !first || !second || blocked || !third:\n        return -1\n    return next * 100 + rest[0] * 10 + rest[1]\n";
    expect_return(source, "incremental_group", "123");
}

#[test]
fn task_group_await_next_reports_empty_without_consuming_the_scope() {
    let source = "fn empty_then_spawn() -> i32 effects [concurrency.task]:\n    let group: TaskGroup[i32] = task_group_new(1)\n    let empty: i32 = unwrap_or(task_group_await_next(group), 7)\n    let spawned: bool = task_group_spawn(group, identity, 4)\n    let next: i32 = unwrap_or(task_group_await_next(group), 0)\n    let cancelled: usize = task_group_cancel(group)\n    if !spawned || cancelled != 0:\n        return -1\n    return empty * 10 + next\n\nfn identity(x: i32) -> i32:\n    return x\n";
    expect_return(source, "empty_then_spawn", "74");
}

#[test]
fn task_group_await_next_receipt_records_scheduler_steps() {
    let source = "fn identity(x: i32) -> i32:\n    return x\n\nfn step() -> i32 effects [concurrency.task]:\n    let group: TaskGroup[i32] = task_group_new(1)\n    let empty: i32 = unwrap_or(task_group_await_next(group), 0)\n    let spawned: bool = task_group_spawn(group, identity, 8)\n    return unwrap_or(task_group_await_next(group), 0)\n";
    let program = parse_human(source).unwrap();
    let output = run_entry_json(
        &program,
        "step",
        "{ \"args\": {}, \"expect\": { \"return\": 8 } }",
    )
    .unwrap();
    assert!(
        output.contains("\"operation\":\"task_group.await_next\",\"target\":\"group\",\"decision\":\"empty\",\"count\":0,\"capacity\":1"),
        "{output}"
    );
    assert!(
        output.contains("\"operation\":\"task_group.await_next\",\"target\":\"group\",\"decision\":\"completed\",\"count\":0,\"capacity\":1"),
        "{output}"
    );
}

#[test]
fn task_group_spawn_wait_applies_bounded_fifo_backpressure() {
    expect_return(BACKPRESSURE_PROGRAM, "rolling_group", "123");
}

#[test]
fn task_group_spawn_wait_receipt_distinguishes_accept_and_progress() {
    let program = parse_human(BACKPRESSURE_PROGRAM).unwrap();
    let output = run_entry_json(
        &program,
        "rolling_group",
        "{ \"args\": {}, \"expect\": { \"return\": 123 } }",
    )
    .unwrap();
    assert_eq!(
        output
            .matches("\"operation\":\"task_group.spawn_wait\",\"target\":\"group\",\"decision\":\"accepted\"")
            .count(),
        2,
        "{output}"
    );
    assert!(
        output.contains("\"operation\":\"task_group.spawn_wait\",\"target\":\"group\",\"decision\":\"capacity_released\",\"count\":2,\"capacity\":2"),
        "{output}"
    );
}

#[test]
fn task_group_admission_rechecks_capacity_after_eager_input_evaluation() {
    let source = "fn identity(x: i32) -> i32:\n    return x\n\nfn reentrant_input() -> i32 effects [concurrency.task]:\n    let group: TaskGroup[i32] = task_group_new(1)\n    let first: bool = task_group_spawn(group, identity, 7)\n    let completed: i32 = unwrap_or(task_group_spawn_wait(group, identity, unwrap_or(task_group_await_next(group), 0)), 0)\n    let remaining: Vec[i32] = task_group_await_all(group)\n    if !first:\n        return -1\n    return completed * 10 + remaining[0]\n";
    expect_return(source, "reentrant_input", "7");
}

#[test]
fn task_group_capacity_rejects_extra_children_without_running_them() {
    expect_return(TASK_GROUP_PROGRAM, "bounded_group", "7");
}

#[test]
fn full_task_group_still_eagerly_evaluates_and_consumes_the_attempted_input() {
    let source = "fn identity(x: i32) -> i32:\n    return x\n\nfn explode() -> i32:\n    return 1 / 0\n\nfn full_group() -> bool effects [concurrency.task]:\n    let group: TaskGroup[i32] = task_group_new(1)\n    let first: bool = task_group_spawn(group, identity, 7)\n    return task_group_spawn(group, identity, explode())\n";
    let program = parse_human(source).unwrap();
    let error = run_entry_json(&program, "full_group", "{ \"args\": {} }").unwrap_err();
    assert!(error.contains("division_by_zero"), "got: {error}");
}

#[test]
fn task_group_cancel_propagates_to_every_pending_child() {
    let source = "fn would_trap(x: i32) -> i32:\n    return x / 0\n\nfn cancel_group() -> usize effects [concurrency.task]:\n    let group: TaskGroup[i32] = task_group_new(2)\n    let first: bool = task_group_spawn(group, would_trap, 1)\n    let second: bool = task_group_spawn(group, would_trap, 2)\n    return task_group_cancel(group)\n";
    expect_return(source, "cancel_group", "2");
}

#[test]
fn empty_task_group_awaits_to_an_empty_vector() {
    let source = "fn empty_group() -> usize effects [concurrency.task]:\n    let group: TaskGroup[i32] = task_group_new(2)\n    let results: Vec[i32] = task_group_await_all(group)\n    return results.len\n";
    expect_return(source, "empty_group", "0");
}

#[test]
fn bounded_channel_is_fifo_and_reports_capacity_without_blocking() {
    expect_return(CHANNEL_PROGRAM, "bounded_fifo", "120");
}

#[test]
fn closed_channel_rejects_new_values_deterministically() {
    expect_return(CHANNEL_PROGRAM, "close_rejects_send", "false");
}

#[test]
fn task_group_runtime_receipt_is_stable_and_records_capacity_decisions() {
    let program = parse_human(TASK_GROUP_PROGRAM).unwrap();
    let input = "{ \"args\": {}, \"expect\": { \"return\": 7 } }";
    let first = run_entry_json(&program, "bounded_group", input).unwrap();
    let second = run_entry_json(&program, "bounded_group", input).unwrap();
    assert_eq!(first, second, "concurrency receipts must be deterministic");
    assert!(
        first.contains("\"schema\": \"forgentex.concurrency_receipt.v1\""),
        "{first}"
    );
    assert!(
        first.contains("\"mode\": \"deterministic_cooperative\""),
        "{first}"
    );
    assert!(
        first.contains("\"operation\":\"task_group.new\",\"target\":\"\",\"decision\":\"active\",\"count\":0,\"capacity\":1"),
        "{first}"
    );
    assert!(
        first.contains("\"operation\":\"task_group.spawn\",\"target\":\"group\",\"decision\":\"accepted\",\"count\":1,\"capacity\":1"),
        "{first}"
    );
    assert!(
        first.contains("\"operation\":\"task_group.spawn\",\"target\":\"group\",\"decision\":\"capacity_exhausted\",\"count\":1,\"capacity\":1"),
        "{first}"
    );
    assert!(
        first.contains("\"operation\":\"task_group.await_all\",\"target\":\"group\",\"decision\":\"completed\",\"count\":1,\"capacity\":1"),
        "{first}"
    );
}

#[test]
fn channel_runtime_receipt_records_depth_close_and_refusals() {
    let source = "fn transitions() -> i32 effects [concurrency.channel]:\n    let channel: Channel[i32] = channel_new(1)\n    let sent: bool = channel_send(channel, 7)\n    let full: bool = channel_send(channel, 8)\n    let value: i32 = unwrap_or(channel_recv(channel), 0)\n    channel_close(channel)\n    let closed: bool = channel_send(channel, 9)\n    let empty: i32 = unwrap_or(channel_recv(channel), 0)\n    if !sent || full || closed:\n        return -1\n    return value + empty\n";
    let program = parse_human(source).unwrap();
    let output = run_entry_json(
        &program,
        "transitions",
        "{ \"args\": {}, \"expect\": { \"return\": 7 } }",
    )
    .unwrap();
    for decision in [
        "\"decision\":\"accepted\",\"count\":1,\"capacity\":1",
        "\"decision\":\"capacity_exhausted\",\"count\":1,\"capacity\":1",
        "\"decision\":\"received\",\"count\":0,\"capacity\":1",
        "\"decision\":\"closed\",\"count\":0,\"capacity\":1",
        "\"decision\":\"empty\",\"count\":0,\"capacity\":1",
    ] {
        assert!(output.contains(decision), "missing {decision}: {output}");
    }
    assert!(output.contains("\"event_count\": 7"), "{output}");
}

#[test]
fn concurrency_effects_must_be_declared() {
    for source in [
        "fn double(x: i32) -> i32:\n    return x * 2\n\nfn missing() -> i32:\n    let task: Task[i32] = task_spawn(double, 2)\n    return task_await(task)\n",
        "fn double(x: i32) -> i32:\n    return x * 2\n\nfn missing() -> i32:\n    let task: Task[i32] = task_spawn_parked(double, 2)\n    let woke: bool = task_wake(task)\n    return task_await(task)\n",
        "fn identity(x: i32) -> i32:\n    return x\n\nfn missing() -> i32:\n    let token: WakeToken = wake_token_new()\n    let task: Task[i32] = task_spawn_on(token, identity, 2)\n    let awakened: usize = wake_token_signal(token)\n    return task_await(task)\n",
        "fn missing() -> usize:\n    let clock: AsyncClock = clock_new()\n    let token: WakeToken = timer_after(clock, 1)\n    return clock_advance(clock, 1)\n",
        "fn identity(x: i32) -> i32:\n    return x\n\nfn missing() -> usize:\n    let group: TaskGroup[i32] = task_group_new(1)\n    let spawned: bool = task_group_spawn(group, identity, 2)\n    return task_group_cancel(group)\n",
    ] {
        let diagnostics = crate::typecheck::check(&parse_human(source).unwrap());
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "undeclared_effect"),
            "got: {diagnostics:?}"
        );
    }
}

#[test]
fn spawned_function_effects_propagate_to_the_caller() {
    for spawn in ["task_spawn", "task_spawn_parked"] {
        let wake = if spawn == "task_spawn_parked" {
            "    let woke: bool = task_wake(task)\n"
        } else {
            ""
        };
        let program = parse_human(&format!(
            "fn effectful(x: i32) -> i32 effects [io.read]:\n    return x\n\nfn missing_io() -> i32 effects [concurrency.task]:\n    let task: Task[i32] = {spawn}(effectful, 2)\n{wake}    return task_await(task)\n"
        ))
        .unwrap();
        let diagnostics = crate::typecheck::check(&program);
        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "undeclared_effect" && diagnostic.message.contains("io.read")
            }),
            "got: {diagnostics:?}"
        );
    }
}

#[test]
fn typed_function_values_propagate_task_effects() {
    let program = parse_human(
        "fn read_value(x: i32) -> i32 effects [io.read]:\n    return x\n\nfn missing_io() -> i32 effects [concurrency.task]:\n    let callable: Fn[i32, i32, effects[io.read]] = read_value\n    let task: Task[i32] = task_spawn(callable, 2)\n    return task_await(task)\n",
    )
    .unwrap();
    let diagnostics = crate::typecheck::check(&program);
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "undeclared_effect" && diagnostic.message.contains("io.read")
        }),
        "got: {diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "task_callable_effect_unknown"),
        "got: {diagnostics:?}"
    );
}

#[test]
fn task_group_spawn_propagates_child_effects() {
    for spawn in [
        "let spawned: bool = task_group_spawn(group, read_value, 2)",
        "let completed: Option[i32] = task_group_spawn_wait(group, read_value, 2)",
    ] {
        let program = parse_human(&format!(
            "fn read_value(x: i32) -> i32 effects [io.read]:\n    return x\n\nfn missing_io() -> i32 effects [concurrency.task]:\n    let group: TaskGroup[i32] = task_group_new(1)\n    {spawn}\n    let values: Vec[i32] = task_group_await_all(group)\n    return values[0]\n"
        ))
        .unwrap();
        let diagnostics = crate::typecheck::check(&program);
        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "undeclared_effect" && diagnostic.message.contains("io.read")
            }),
            "got: {diagnostics:?}"
        );
    }
}

#[test]
fn token_waiting_task_propagates_child_effects() {
    let program = parse_human(
        "fn read_value(x: i32) -> i32 effects [io.read]:\n    return x\n\nfn missing_io() -> i32 effects [concurrency.task]:\n    let token: WakeToken = wake_token_new()\n    let task: Task[i32] = task_spawn_on(token, read_value, 2)\n    let awakened: usize = wake_token_signal(token)\n    return task_await(task)\n",
    )
    .unwrap();
    let diagnostics = crate::typecheck::check(&program);
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "undeclared_effect" && diagnostic.message.contains("io.read")
        }),
        "got: {diagnostics:?}"
    );
}

#[test]
fn task_handles_are_affine_and_cannot_be_awaited_twice() {
    let program = parse_human(
        "fn double(x: i32) -> i32:\n    return x * 2\n\nfn twice() -> i32 effects [concurrency.task]:\n    let task: Task[i32] = task_spawn(double, 2)\n    let first: i32 = task_await(task)\n    return first + task_await(task)\n",
    )
    .unwrap();
    let diagnostics = crate::typecheck::check(&program);
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "use_after_move"),
        "got: {diagnostics:?}"
    );
}

#[test]
fn task_groups_are_affine_and_cannot_be_used_after_cancellation() {
    let program = parse_human(
        "fn identity(x: i32) -> i32:\n    return x\n\nfn reused() -> usize effects [concurrency.task]:\n    let group: TaskGroup[i32] = task_group_new(1)\n    let spawned: bool = task_group_spawn(group, identity, 1)\n    let cancelled: usize = task_group_cancel(group)\n    let values: Vec[i32] = task_group_await_all(group)\n    return cancelled + values.len\n",
    )
    .unwrap();
    let diagnostics = crate::typecheck::check(&program);
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "use_after_move"),
        "got: {diagnostics:?}"
    );
}

#[test]
fn owned_task_inputs_move_into_the_task() {
    for spawn in ["task_spawn", "task_spawn_parked"] {
        let program = parse_human(&format!(
            "fn size(v: Vec[i32]) -> usize:\n    return v.len\n\nfn moved() -> usize effects [alloc, concurrency.task]:\n    let values: Vec[i32] = vec_new()\n    push(values, 1)\n    let task: Task[usize] = {spawn}(size, values)\n    let cancelled: bool = task_cancel(task)\n    return values.len\n"
        ))
        .unwrap();
        let diagnostics = crate::typecheck::check(&program);
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "use_after_move"),
            "got: {diagnostics:?}"
        );
    }
}

#[test]
fn owned_task_group_inputs_move_into_the_group() {
    let program = parse_human(
        "fn size(v: Vec[i32]) -> usize:\n    return v.len\n\nfn moved() -> usize effects [alloc, concurrency.task]:\n    let values: Vec[i32] = vec_new()\n    push(values, 1)\n    let group: TaskGroup[usize] = task_group_new(1)\n    let spawned: bool = task_group_spawn(group, size, values)\n    let results: Vec[usize] = task_group_await_all(group)\n    return values.len + results[0]\n",
    )
    .unwrap();
    let diagnostics = crate::typecheck::check(&program);
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "use_after_move"),
        "got: {diagnostics:?}"
    );
}

#[test]
fn owned_backpressured_task_inputs_move_into_the_group() {
    let program = parse_human(
        "fn size(v: Vec[i32]) -> usize:\n    return v.len\n\nfn moved() -> usize effects [alloc, concurrency.task]:\n    let values: Vec[i32] = vec_new()\n    push(values, 1)\n    let group: TaskGroup[usize] = task_group_new(1)\n    let completed: Option[usize] = task_group_spawn_wait(group, size, values)\n    return values.len + task_group_cancel(group)\n",
    )
    .unwrap();
    let diagnostics = crate::typecheck::check(&program);
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "use_after_move"),
        "got: {diagnostics:?}"
    );
}

#[test]
fn task_group_rejects_a_callable_with_the_wrong_result_type() {
    for spawn in [
        "let spawned: bool = task_group_spawn(group, truth, 1)",
        "let completed: Option[i32] = task_group_spawn_wait(group, truth, 1)",
    ] {
        let program = parse_human(&format!(
            "fn truth(x: i32) -> bool:\n    return x > 0\n\nfn wrong() -> usize effects [concurrency.task]:\n    let group: TaskGroup[i32] = task_group_new(1)\n    {spawn}\n    return task_group_cancel(group)\n"
        ))
        .unwrap();
        let diagnostics = crate::typecheck::check(&program);
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "task_group_output_type"),
            "got: {diagnostics:?}"
        );
    }
}

#[test]
fn invalid_channel_capacity_traps_instead_of_blocking_or_allocating() {
    let program = parse_human(
        "fn bad() -> bool effects [concurrency.channel]:\n    let channel: Channel[i32] = channel_new(0)\n    return channel_send(channel, 1)\n",
    )
    .unwrap();
    let error = run_entry_json(&program, "bad", "{ \"args\": {} }").unwrap_err();
    assert!(error.contains("channel_capacity"), "got: {error}");
}

#[test]
fn task_group_capacity_is_positive_and_hard_bounded() {
    for capacity in [0, 1025] {
        let program = parse_human(&format!(
            "fn bad() -> usize effects [concurrency.task]:\n    let group: TaskGroup[i32] = task_group_new({capacity})\n    return task_group_cancel(group)\n"
        ))
        .unwrap();
        let error = run_entry_json(&program, "bad", "{ \"args\": {} }").unwrap_err();
        assert!(error.contains("task_group_capacity"), "got: {error}");
    }
}

#[test]
fn concurrency_types_and_calls_roundtrip_in_both_syntaxes() {
    let program = parse_human(&format!(
        "{TASK_PROGRAM}\n{PARKED_TASK_PROGRAM}\n{WAKE_TOKEN_PROGRAM}\n{TIMER_PROGRAM}\n{TIMER_CONTROL_PROGRAM}\n{PERIODIC_TIMER_PROGRAM}\n{SELECT_PROGRAM}\n{REACTOR_WAIT_PROGRAM}\n{HETEROGENEOUS_READINESS_PROGRAM}\n{CHANNEL_PROGRAM}\n{TASK_GROUP_PROGRAM}\n{BACKPRESSURE_PROGRAM}"
    ))
    .unwrap();
    assert!(program.canonical_eq(&parse_agent(&format_agent(&program)).unwrap()));
    assert!(program.canonical_eq(&parse_human(&format_human(&program)).unwrap()));
}

#[test]
fn concurrency_programs_typecheck_when_effects_and_types_match() {
    for source in [
        TASK_PROGRAM,
        PARKED_TASK_PROGRAM,
        WAKE_TOKEN_PROGRAM,
        TIMER_PROGRAM,
        TIMER_CONTROL_PROGRAM,
        PERIODIC_TIMER_PROGRAM,
        SELECT_PROGRAM,
        REACTOR_WAIT_PROGRAM,
        HETEROGENEOUS_READINESS_PROGRAM,
        CHANNEL_PROGRAM,
        TASK_GROUP_PROGRAM,
        BACKPRESSURE_PROGRAM,
    ] {
        let diagnostics = crate::typecheck::check(&parse_human(source).unwrap());
        assert!(diagnostics.is_empty(), "unexpected: {diagnostics:?}");
    }
}
