// Integer semantics: what arithmetic does at the edges, at each declared width.
//
// The property under test throughout is that a width means something. Before
// this, the interpreter checked every operation at 64 bits regardless of how it
// was declared — so an `i32` function could return 4000000000 — and the C
// backend emitted C's own operators, leaving signed overflow, division by zero,
// `MIN / -1`, and out-of-range shifts undefined. The two therefore disagreed on
// exactly the cases a numeric language most needs a defined answer for.
//
// `docs/numeric_determinism.md` states the rules these tests hold the
// implementation to.

use crate::interp::run_entry_json;
use crate::parser::parse_human;

/// Run `body` as a two-argument entry over `a` and `b`, returning the JSON
/// result or the trap.
fn run(params: &str, ret: &str, body: &str, a: i64, b: i64) -> Result<String, String> {
    let source = format!("fn probe({params}) -> {ret}:\n    {body}\n");
    let program = parse_human(&source).expect("source should parse");
    let input = format!("{{ \"args\": {{ \"a\": {a}, \"b\": {b} }} }}");
    run_entry_json(&program, "probe", &input)
}

fn trap_of(result: Result<String, String>) -> String {
    result.expect_err("this operation should have trapped")
}

#[test]
fn integer_overflow_traps_at_the_declared_width_not_at_sixty_four() {
    // 2000000000 + 2000000000 fits an i64 comfortably and an i32 not at all.
    // Checking at 64 bits let the sum through and handed an `i32` function a
    // value no `i32` can hold.
    let trapped = trap_of(run(
        "a: i32, b: i32",
        "i32",
        "return a + b",
        2_000_000_000,
        2_000_000_000,
    ));
    assert!(trapped.contains("integer_overflow"), "{trapped}");
    assert!(
        trapped.contains("i32"),
        "the width has to be in the message: {trapped}"
    );

    // The same arithmetic declared at the width that holds it is not an error.
    let ok = run(
        "a: i64, b: i64",
        "i64",
        "return a + b",
        2_000_000_000,
        2_000_000_000,
    )
    .expect("i64 holds the sum");
    assert!(ok.contains("4000000000"), "{ok}");
}

#[test]
fn every_width_is_checked_at_its_own_bound() {
    // A width is not a documentation comment: the narrow types have to be
    // enforced too, or `i8` is just `i64` with a smaller name.
    for (params, ret, a, b) in [
        ("a: i8, b: i8", "i8", 100, 100),
        ("a: i16, b: i16", "i16", 30_000, 30_000),
        ("a: i32, b: i32", "i32", 2_000_000_000, 2_000_000_000),
    ] {
        let trapped = trap_of(run(params, ret, "return a + b", a, b));
        assert!(trapped.contains("integer_overflow"), "{params}: {trapped}");
    }

    // Unsigned types have a floor as well as a ceiling, and subtraction is the
    // operation that finds it.
    let trapped = trap_of(run("a: u8, b: u8", "u8", "return a - b", 3, 5));
    assert!(trapped.contains("integer_overflow"), "{trapped}");
}

#[test]
fn a_value_exactly_at_the_boundary_is_not_an_overflow() {
    // The guard against a check that simply rejects large numbers: every one of
    // these sits on the edge and must be allowed through.
    for (params, ret, a, b, expected) in [
        ("a: i8, b: i8", "i8", 100, 27, "127"),
        ("a: i8, b: i8", "i8", -100, -28, "-128"),
        ("a: i32, b: i32", "i32", 2_147_483_647, 0, "2147483647"),
        ("a: u8, b: u8", "u8", 255, 0, "255"),
    ] {
        let ok = run(params, ret, "return a + b", a, b)
            .unwrap_or_else(|err| panic!("{params} with {a}, {b} should not trap: {err}"));
        assert!(ok.contains(expected), "{params}: {ok}");
    }
}

#[test]
fn division_by_zero_and_by_negative_one_at_the_smallest_value_both_trap() {
    let trapped = trap_of(run("a: i32, b: i32", "i32", "return a / b", 10, 0));
    assert!(trapped.contains("division_by_zero"), "{trapped}");

    let trapped = trap_of(run("a: i32, b: i32", "i32", "return a % b", 10, 0));
    assert!(trapped.contains("division_by_zero"), "{trapped}");

    // The quotient of the smallest value by -1 is one past the maximum.
    let trapped = trap_of(run(
        "a: i32, b: i32",
        "i32",
        "return a / b",
        i32::MIN as i64,
        -1,
    ));
    assert!(trapped.contains("integer_overflow"), "{trapped}");

    // The remainder is zero and would slip past a check that only looked at the
    // result, but the operation overflows just the same — which is what C and
    // Rust both say.
    let trapped = trap_of(run(
        "a: i32, b: i32",
        "i32",
        "return a % b",
        i32::MIN as i64,
        -1,
    ));
    assert!(trapped.contains("integer_overflow"), "{trapped}");
}

#[test]
fn a_shift_amount_must_be_inside_the_declared_width() {
    // Checking the amount against 64 rather than the declared width made
    // `1i32 << 40` produce 1099511627776 — a number no `i32` holds, from an
    // operation C leaves undefined.
    let trapped = trap_of(run("a: i32, b: i32", "i32", "return a << b", 1, 40));
    assert!(trapped.contains("invalid_shift"), "{trapped}");
    assert!(
        trapped.contains("32"),
        "the width belongs in the message: {trapped}"
    );

    let trapped = trap_of(run("a: i32, b: i32", "i32", "return a >> b", 1, 32));
    assert!(trapped.contains("invalid_shift"), "{trapped}");

    let trapped = trap_of(run("a: i32, b: i32", "i32", "return a << b", 1, -1));
    assert!(trapped.contains("invalid_shift"), "{trapped}");

    // One below the width is the largest legal shift.
    let ok = run("a: i32, b: i32", "i32", "return a << b", 1, 31).expect("31 is in range for i32");
    assert!(ok.contains("-2147483648"), "{ok}");
}

#[test]
fn a_shift_discards_the_bits_that_leave_rather_than_trapping() {
    // A shift moves a bit pattern, so bits leaving the top are gone — that is
    // what the operation means, and it is what the emitted C does. Only the
    // amount is an error.
    let ok = run("a: i32, b: i32", "i32", "return a << b", 1_073_741_824, 2)
        .expect("shifting bits off the top is not an overflow");
    assert!(ok.contains("\"return\": 0"), "{ok}");
}

#[test]
fn the_generated_c_traps_wherever_the_interpreter_traps() {
    // Both halves of the contract in one test: the same program, the same
    // inputs, and the same verdict from the interpreter and from the compiled C.
    let Some(toolchain) = crate::native_build::detect_toolchain() else {
        eprintln!("skipping: no host C toolchain");
        return;
    };

    let root = std::env::temp_dir().join(format!(
        "forgentex_intsem_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Forgentex.toml"),
        "[package]\nname = \"intsem\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    let source = "export fn add_i32(a: i32, b: i32) -> i32:\n    return a + b\n";
    std::fs::write(root.join("src/m.fgx"), source).unwrap();

    // The interpreter's verdict.
    let program = parse_human(source).unwrap();
    let overflowing = "{ \"args\": { \"a\": 2000000000, \"b\": 2000000000 } }";
    let trapped = run_entry_json(&program, "add_i32", overflowing)
        .expect_err("the interpreter should trap on i32 overflow");
    assert!(trapped.contains("integer_overflow"), "{trapped}");

    // The compiled C's verdict, on the same inputs.
    let (header, generated) = crate::c_backend::generate_c_for_project(&root).unwrap();
    assert!(
        generated.contains("fgx_checked_i32_add"),
        "the addition must be lowered through the width-checked helper:\n{generated}"
    );
    let include = generated
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("#include \"")
                .and_then(|rest| rest.strip_suffix('"'))
        })
        .expect("the generated source includes its own header");
    std::fs::write(root.join(include), &header).unwrap();
    std::fs::write(root.join("generated.c"), &generated).unwrap();
    std::fs::write(
        root.join("harness.c"),
        format!("#include \"{include}\"\nint main(void) {{ add_i32(2000000000, 2000000000); return 0; }}\n"),
    )
    .unwrap();

    let executable = root.join(if cfg!(windows) { "p.exe" } else { "p" });
    let argv = vec![
        toolchain.name.clone(),
        "-std=c11".to_string(),
        root.join("harness.c").display().to_string(),
        root.join("generated.c").display().to_string(),
        "-I".to_string(),
        root.display().to_string(),
        "-o".to_string(),
        executable.display().to_string(),
    ];
    let compiled =
        crate::native_build::run_with_path(&argv, std::path::Path::new(&toolchain.name).parent())
            .expect("failed to invoke the host C compiler");
    assert!(
        compiled.status.success(),
        "generated C did not compile:\n{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let run = std::process::Command::new(&executable)
        .output()
        .expect("failed to run the compiled program");
    assert!(
        !run.status.success(),
        "the compiled C must trap where the interpreter traps, but it exited cleanly"
    );
    let _ = std::fs::remove_dir_all(&root);
}
