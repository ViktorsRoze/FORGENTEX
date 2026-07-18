use std::fs;

use crate::c_backend::build_c_project;
use crate::cfg::build_cfg;
use crate::format::{format_agent, format_human};
use crate::interp::run_entry_json;
use crate::parser::{parse_agent, parse_human};

// A data-less (C-like) enum plus functions that construct, pass, and compare
// its variants. This is the whole lang.2 (part 1) surface: declaration,
// variant values, and equality. `match` and payload-carrying variants land next.
const COLORS: &str = "enum Color:\n    Red\n    Green\n    Blue\n\nfn classify(c: Color) -> i32:\n    if c == Color::Red:\n        return 0\n    if c == Color::Green:\n        return 1\n    return 2\n\nfn green_code() -> i32:\n    return classify(Color::Green)\n\nfn red_code() -> i32:\n    return classify(Color::Red)\n";

#[test]
fn enum_roundtrips_both_syntaxes() {
    let program = parse_human(COLORS).unwrap();
    // .fgx -> AST -> .fgai -> AST is lossless (canonical AST equality).
    let from_agent = parse_agent(&format_agent(&program)).unwrap();
    assert!(program.canonical_eq(&from_agent));
    // Human formatting is idempotent at the AST level.
    let reparsed = parse_human(&format_human(&program)).unwrap();
    assert!(program.canonical_eq(&reparsed));
}

#[test]
fn agent_uses_dense_enum_keyword() {
    let agent = format_agent(&parse_human(COLORS).unwrap());
    assert!(
        agent.contains("EN Color { Red; Green; Blue }"),
        "agent:\n{agent}"
    );
    // Variant paths are preserved verbatim in the dense surface.
    assert!(agent.contains("Color::Green"), "agent:\n{agent}");
    assert!(
        !agent.contains("enum Color"),
        "agent must not use the human keyword"
    );
}

#[test]
fn enum_is_clean_under_the_type_checker() {
    // A valid enum program produces no diagnostics — variant values type as
    // their enum, and same-enum equality is allowed.
    let diagnostics = crate::typecheck::check(&parse_human(COLORS).unwrap());
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn variant_equality_runs_in_the_interpreter() {
    let program = parse_human(COLORS).unwrap();
    for (entry, expected) in [("green_code", 1), ("red_code", 0)] {
        let input = format!("{{ \"args\": {{}}, \"expect\": {{ \"return\": {expected} }} }}");
        let output = run_entry_json(&program, entry, &input).unwrap();
        assert!(
            output.contains("\"status\": \"pass\""),
            "{entry}:\n{output}"
        );
    }
}

#[test]
fn unknown_variant_is_rejected() {
    let src =
        "enum Color:\n    Red\n\nfn f() -> i32:\n    let c: Color = Color::Purple\n    return 0\n";
    let diagnostics = crate::typecheck::check(&parse_human(src).unwrap());
    assert!(
        diagnostics.iter().any(|d| d.code == "unknown_enum_variant"),
        "got: {diagnostics:?}"
    );
}

#[test]
fn unknown_enum_is_rejected() {
    let src = "fn f() -> i32:\n    let s: i32 = 0\n    if Shape::Circle == Shape::Circle:\n        return 1\n    return 0\n";
    let diagnostics = crate::typecheck::check(&parse_human(src).unwrap());
    assert!(
        diagnostics.iter().any(|d| d.code == "unknown_enum"),
        "got: {diagnostics:?}"
    );
}

#[test]
fn c_backend_emits_enum() {
    let dir = std::env::temp_dir().join(format!("forgentex_{}_enum_c", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"enumdemo\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/m.fgx"),
        COLORS.replace("fn green_code", "export fn green_code"),
    )
    .unwrap();
    let artifacts = build_c_project(&dir, &dir.join("out")).unwrap();
    let header = fs::read_to_string(&artifacts.header).unwrap();
    assert!(
        header.contains("typedef enum { Color_Red, Color_Green, Color_Blue } Color;"),
        "expected enum typedef in header:\n{header}"
    );
    let source = fs::read_to_string(&artifacts.source).unwrap();
    assert!(
        source.contains("Color_Green"),
        "expected variant constant in C:\n{source}"
    );
    let _ = fs::remove_dir_all(&dir);
}

// --- match over enums (closes lang.1 + lang.2 pattern matching) ---

// Exhaustive match over all three variants; arms assign a local so the function
// still has a single definite return.
const MATCH_COLORS: &str = "enum Color:\n    Red\n    Green\n    Blue\n\nfn classify(c: Color) -> i32:\n    let result: i32 = -1\n    match c:\n        case Color::Red:\n            result = 0\n        case Color::Green:\n            result = 1\n        case Color::Blue:\n            result = 2\n    return result\n\nfn green_code() -> i32:\n    return classify(Color::Green)\n\nfn blue_code() -> i32:\n    return classify(Color::Blue)\n";

#[test]
fn match_roundtrips_with_dense_keywords() {
    let program = parse_human(MATCH_COLORS).unwrap();
    let agent = format_agent(&program);
    assert!(agent.contains("MT c {"), "agent:\n{agent}");
    assert!(agent.contains("Color::Red => {"), "agent:\n{agent}");
    assert!(
        !agent.contains("match"),
        "agent must not use the human keyword"
    );
    assert!(
        !agent.contains("case "),
        "agent must not use the human arm keyword"
    );
    // .fgx -> AST -> .fgai -> AST and .fgx -> human -> AST are both lossless.
    assert!(program.canonical_eq(&parse_agent(&agent).unwrap()));
    assert!(program.canonical_eq(&parse_human(&format_human(&program)).unwrap()));
}

#[test]
fn match_dispatches_in_the_interpreter() {
    let program = parse_human(MATCH_COLORS).unwrap();
    for (entry, expected) in [("green_code", 1), ("blue_code", 2)] {
        let input = format!("{{ \"args\": {{}}, \"expect\": {{ \"return\": {expected} }} }}");
        let output = run_entry_json(&program, entry, &input).unwrap();
        assert!(
            output.contains("\"status\": \"pass\""),
            "{entry}:\n{output}"
        );
    }
}

#[test]
fn match_is_an_n_way_branch_in_the_cfg_for_soundness() {
    // The borrow checker runs over the CFG, so each arm must be its own block
    // reachable from the scrutinee branch — otherwise arms go unanalyzed.
    let with_match = build_cfg(
        parse_human(MATCH_COLORS)
            .unwrap()
            .find_function("classify")
            .unwrap(),
    );
    let trivial = build_cfg(
        parse_human("fn t(c: Color) -> i32:\n    return 0\n")
            .unwrap()
            .find_function("t")
            .unwrap(),
    );
    // Three arms + a join add several blocks beyond a straight-line function.
    assert!(
        with_match.blocks.len() >= trivial.blocks.len() + 3,
        "a 3-arm match must add arm blocks to the CFG (match={}, trivial={})",
        with_match.blocks.len(),
        trivial.blocks.len()
    );
}

#[test]
fn non_exhaustive_match_is_rejected() {
    // Missing `Blue`, no wildcard.
    let src = "enum Color:\n    Red\n    Green\n    Blue\n\nfn f(c: Color) -> i32:\n    match c:\n        case Color::Red:\n            return 0\n        case Color::Green:\n            return 1\n    return 2\n";
    let diagnostics = crate::typecheck::check(&parse_human(src).unwrap());
    let diag = diagnostics
        .iter()
        .find(|d| d.code == "non_exhaustive_match")
        .unwrap_or_else(|| panic!("expected non_exhaustive_match, got: {diagnostics:?}"));
    assert!(
        diag.message.contains("Blue"),
        "should name the missing variant: {diag:?}"
    );
}

#[test]
fn wildcard_makes_a_partial_match_exhaustive() {
    let src = "enum Color:\n    Red\n    Green\n    Blue\n\nfn f(c: Color) -> i32:\n    match c:\n        case Color::Red:\n            return 0\n        case _:\n            return 9\n";
    let diagnostics = crate::typecheck::check(&parse_human(src).unwrap());
    assert!(
        diagnostics.is_empty(),
        "wildcard should satisfy exhaustiveness: {diagnostics:?}"
    );
}

#[test]
fn match_on_a_non_enum_is_rejected() {
    let src = "fn f(n: i32) -> i32:\n    match n:\n        case _:\n            return 0\n";
    let diagnostics = crate::typecheck::check(&parse_human(src).unwrap());
    assert!(
        diagnostics.iter().any(|d| d.code == "match_not_enum"),
        "got: {diagnostics:?}"
    );
}

#[test]
fn c_backend_lowers_match_to_a_switch() {
    let dir = std::env::temp_dir().join(format!("forgentex_{}_match_c", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"matchdemo\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/m.fgx"),
        MATCH_COLORS.replace("fn green_code", "export fn green_code"),
    )
    .unwrap();
    let artifacts = build_c_project(&dir, &dir.join("out")).unwrap();
    let source = fs::read_to_string(&artifacts.source).unwrap();
    assert!(
        source.contains("switch ("),
        "expected switch in C:\n{source}"
    );
    assert!(
        source.contains("case Color_Red:"),
        "expected variant case in C:\n{source}"
    );
    let _ = fs::remove_dir_all(&dir);
}

// --- payload-carrying variants + binding patterns (the data half of lang.2) ---

// Variants carry an optional single payload; a struct payload expresses the
// multi-field case. Binding patterns destructure the payload into the arm.
const SHAPES: &str = "enum Shape:\n    Circle(i32)\n    Square(i32)\n    Empty\n\nfn area(s: Shape) -> i32:\n    let out: i32 = 0\n    match s:\n        case Shape::Circle(r):\n            out = r * r * 3\n        case Shape::Square(side):\n            out = side * side\n        case Shape::Empty:\n            out = 0\n    return out\n\nfn circle_area() -> i32:\n    return area(Shape::Circle(2))\n\nfn square_area() -> i32:\n    return area(Shape::Square(3))\n";

#[test]
fn payload_enum_roundtrips_with_dense_keywords() {
    let program = parse_human(SHAPES).unwrap();
    let agent = format_agent(&program);
    // Payload types survive in both the declaration and the binding pattern.
    assert!(
        agent.contains("EN Shape { Circle(i32); Square(i32); Empty }"),
        "agent:\n{agent}"
    );
    assert!(agent.contains("Shape::Circle(r) => {"), "agent:\n{agent}");
    assert!(program.canonical_eq(&parse_agent(&agent).unwrap()));
    assert!(program.canonical_eq(&parse_human(&format_human(&program)).unwrap()));
}

#[test]
fn payload_construction_and_binding_run_in_the_interpreter() {
    let program = parse_human(SHAPES).unwrap();
    // circle: 2*2*3 = 12 ; square: 3*3 = 9
    for (entry, expected) in [("circle_area", 12), ("square_area", 9)] {
        let input = format!("{{ \"args\": {{}}, \"expect\": {{ \"return\": {expected} }} }}");
        let output = run_entry_json(&program, entry, &input).unwrap();
        assert!(
            output.contains("\"status\": \"pass\""),
            "{entry}:\n{output}"
        );
    }
}

#[test]
fn payload_program_is_clean_under_the_type_checker() {
    let diagnostics = crate::typecheck::check(&parse_human(SHAPES).unwrap());
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn payload_type_mismatch_is_rejected() {
    // Circle carries an i32, constructed with a string.
    let src = "enum Shape:\n    Circle(i32)\n\nfn f() -> Shape:\n    return Shape::Circle(\"x\")\n";
    let diagnostics = crate::typecheck::check(&parse_human(src).unwrap());
    assert!(
        diagnostics.iter().any(|d| d.code == "variant_payload_type"),
        "got: {diagnostics:?}"
    );
}

#[test]
fn variant_arity_is_checked() {
    // Empty is data-less but constructed with an argument.
    let src =
        "enum Shape:\n    Circle(i32)\n    Empty\n\nfn f() -> Shape:\n    return Shape::Empty(5)\n";
    let diagnostics = crate::typecheck::check(&parse_human(src).unwrap());
    assert!(
        diagnostics.iter().any(|d| d.code == "variant_arity"),
        "got: {diagnostics:?}"
    );
}

#[test]
fn binding_on_a_data_less_variant_is_rejected() {
    let src = "enum Shape:\n    Circle(i32)\n    Empty\n\nfn f(s: Shape) -> i32:\n    match s:\n        case Shape::Circle(r):\n            return r\n        case Shape::Empty(x):\n            return x\n";
    let diagnostics = crate::typecheck::check(&parse_human(src).unwrap());
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == "binding_on_data_less_variant"),
        "got: {diagnostics:?}"
    );
}

#[test]
fn binding_type_flows_into_the_arm() {
    // `r` is bound as i32; using it as if it were a bool must be a type error.
    let src = "enum Shape:\n    Circle(i32)\n\nfn f(s: Shape) -> i32:\n    match s:\n        case Shape::Circle(r):\n            if r:\n                return 1\n    return 0\n";
    let diagnostics = crate::typecheck::check(&parse_human(src).unwrap());
    assert!(
        diagnostics.iter().any(|d| d.code == "if_not_bool"),
        "binding should type as i32, making `if r` ill-typed: {diagnostics:?}"
    );
}

#[test]
fn c_backend_lowers_payload_enum_to_a_tagged_union() {
    let dir = std::env::temp_dir().join(format!("forgentex_{}_payload_c", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"payloaddemo\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/m.fgx"),
        SHAPES.replace("fn circle_area", "export fn circle_area"),
    )
    .unwrap();
    let artifacts = build_c_project(&dir, &dir.join("out")).unwrap();
    let header = fs::read_to_string(&artifacts.header).unwrap();
    // A discriminant plus a union of the payloads keyed by variant name.
    assert!(
        header.contains("enum { Shape_Circle, Shape_Square, Shape_Empty } tag;"),
        "header:\n{header}"
    );
    assert!(header.contains("int32_t Circle;"), "header:\n{header}");
    assert!(header.contains("} data;"), "header:\n{header}");

    let source = fs::read_to_string(&artifacts.source).unwrap();
    // Match switches on the discriminant and binds the payload out of the union.
    assert!(
        source.contains("switch (_fgx_scrut.tag)"),
        "source:\n{source}"
    );
    assert!(
        source.contains("int32_t r = _fgx_scrut.data.Circle;"),
        "source:\n{source}"
    );
    // Construction is a tagged-union compound literal.
    assert!(
        source.contains("(Shape){ .tag = Shape_Circle, .data.Circle = (2) }"),
        "source:\n{source}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn payload_enum_c_compiles_under_a_host_compiler() {
    // Compiler-gated: skips when no host C compiler is present (e.g. a bare
    // dev box); under CI it proves the generated tagged-union C is valid C11.
    let Some(toolchain) = crate::native_build::detect_toolchain() else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("forgentex_{}_payload_cc", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("Forgentex.toml"),
        "[package]\nname = \"payloadcc\"\nversion = \"0.1.0\"\nroot = \"src\"\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/m.fgx"),
        SHAPES.replace("fn circle_area", "export fn circle_area"),
    )
    .unwrap();
    let artifacts = build_c_project(&dir, &dir.join("out")).unwrap();
    let obj = dir.join("out").join("m.o");
    // Use the same compiler-directory PATH handling as native builds and FFI.
    // A full-path `$CC` such as w64devkit's gcc needs this for `as`/`ld`.
    let argv = vec![
        toolchain.name.clone(),
        "-std=c11".to_string(),
        "-c".to_string(),
        artifacts.source.display().to_string(),
        "-I".to_string(),
        dir.join("out").display().to_string(),
        "-o".to_string(),
        obj.display().to_string(),
    ];
    let status =
        crate::native_build::run_with_path(&argv, std::path::Path::new(&toolchain.name).parent())
            .expect("failed to invoke host C compiler")
            .status;
    assert!(
        status.success(),
        "generated tagged-union C should compile cleanly under {}",
        toolchain.name
    );
    let _ = fs::remove_dir_all(&dir);
}
