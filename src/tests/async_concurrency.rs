use crate::format::{format_agent, format_human};
use crate::interp::run_entry_json;
use crate::parser::{parse_agent, parse_human};

const TASK_PROGRAM: &str = "fn double(x: i32) -> i32:\n    return x * 2\n\nfn run_task() -> i32 effects [concurrency.task]:\n    let task: Task[i32] = task_spawn(double, 21)\n    return task_await(task)\n\nfn cancel_task() -> bool effects [concurrency.task]:\n    let task: Task[i32] = task_spawn(double, 21)\n    return task_cancel(task)\n";

const CHANNEL_PROGRAM: &str = "fn bounded_fifo() -> i32 effects [concurrency.channel]:\n    let channel: Channel[i32] = channel_new(2)\n    let first_sent: bool = channel_send(channel, 10)\n    let second_sent: bool = channel_send(channel, 20)\n    let overflow_sent: bool = channel_send(channel, 30)\n    if !first_sent || !second_sent || overflow_sent:\n        return -1\n    let first: i32 = unwrap_or(channel_recv(channel), 0)\n    let second: i32 = unwrap_or(channel_recv(channel), 0)\n    return first * 10 + second\n\nfn close_rejects_send() -> bool effects [concurrency.channel]:\n    let channel: Channel[i32] = channel_new(1)\n    channel_close(channel)\n    return channel_send(channel, 1)\n";

const TASK_GROUP_PROGRAM: &str = "fn identity(x: i32) -> i32:\n    return x\n\nfn ordered_group() -> i32 effects [concurrency.task]:\n    let group: TaskGroup[i32] = task_group_new(3)\n    let first: bool = task_group_spawn(group, identity, 1)\n    let second: bool = task_group_spawn(group, identity, 2)\n    let third: bool = task_group_spawn(group, identity, 3)\n    if !first || !second || !third:\n        return -1\n    let results: Vec[i32] = task_group_await_all(group)\n    return results[0] * 100 + results[1] * 10 + results[2]\n\nfn bounded_group() -> i32 effects [concurrency.task]:\n    let group: TaskGroup[i32] = task_group_new(1)\n    let first: bool = task_group_spawn(group, identity, 7)\n    let overflow: bool = task_group_spawn(group, identity, 9)\n    let results: Vec[i32] = task_group_await_all(group)\n    if !first || overflow:\n        return -1\n    return results[0]\n";

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
    expect_return(TASK_PROGRAM, "run_task", "42");
}

#[test]
fn pending_task_can_be_cancelled_without_running() {
    expect_return(TASK_PROGRAM, "cancel_task", "true");
}

#[test]
fn task_group_awaits_children_in_deterministic_spawn_order() {
    expect_return(TASK_GROUP_PROGRAM, "ordered_group", "123");
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
fn concurrency_effects_must_be_declared() {
    for source in [
        "fn double(x: i32) -> i32:\n    return x * 2\n\nfn missing() -> i32:\n    let task: Task[i32] = task_spawn(double, 2)\n    return task_await(task)\n",
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
    let program = parse_human(
        "fn effectful(x: i32) -> i32 effects [io.read]:\n    return x\n\nfn missing_io() -> i32 effects [concurrency.task]:\n    let task: Task[i32] = task_spawn(effectful, 2)\n    return task_await(task)\n",
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
    let program = parse_human(
        "fn read_value(x: i32) -> i32 effects [io.read]:\n    return x\n\nfn missing_io() -> i32 effects [concurrency.task]:\n    let group: TaskGroup[i32] = task_group_new(1)\n    let spawned: bool = task_group_spawn(group, read_value, 2)\n    let values: Vec[i32] = task_group_await_all(group)\n    return values[0]\n",
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
    let program = parse_human(
        "fn size(v: Vec[i32]) -> usize:\n    return v.len\n\nfn moved() -> usize effects [alloc, concurrency.task]:\n    let values: Vec[i32] = vec_new()\n    push(values, 1)\n    let task: Task[usize] = task_spawn(size, values)\n    return values.len + task_await(task)\n",
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
fn task_group_rejects_a_callable_with_the_wrong_result_type() {
    let program = parse_human(
        "fn truth(x: i32) -> bool:\n    return x > 0\n\nfn wrong() -> usize effects [concurrency.task]:\n    let group: TaskGroup[i32] = task_group_new(1)\n    let spawned: bool = task_group_spawn(group, truth, 1)\n    return task_group_cancel(group)\n",
    )
    .unwrap();
    let diagnostics = crate::typecheck::check(&program);
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "task_group_output_type"),
        "got: {diagnostics:?}"
    );
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
        "{TASK_PROGRAM}\n{CHANNEL_PROGRAM}\n{TASK_GROUP_PROGRAM}"
    ))
    .unwrap();
    assert!(program.canonical_eq(&parse_agent(&format_agent(&program)).unwrap()));
    assert!(program.canonical_eq(&parse_human(&format_human(&program)).unwrap()));
}

#[test]
fn concurrency_programs_typecheck_when_effects_and_types_match() {
    for source in [TASK_PROGRAM, CHANNEL_PROGRAM, TASK_GROUP_PROGRAM] {
        let diagnostics = crate::typecheck::check(&parse_human(source).unwrap());
        assert!(diagnostics.is_empty(), "unexpected: {diagnostics:?}");
    }
}
