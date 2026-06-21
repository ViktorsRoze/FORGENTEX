use std::fs;
use std::path::{Path, PathBuf};

use crate::abi::{abi_json, abi_text, analyze_abi, generate_header};
use crate::asmcheck::{analyze_assembly, asm_check_json, review_text};
use crate::backend::{
    backend_catalog_json, backend_catalog_text, backend_ir_json, backend_ir_text,
    build_project_with_backend, frontend_contract_json, generate_backend_ir,
};
use crate::borrow::analyze_borrows;
use crate::borrow_conformance::run_borrow_conformance;
use crate::cfg::{back_edges, build_cfg, cfg_to_json, loop_carried_live};
use crate::diag::diagnostics_to_json;
use crate::diagnostic_catalog::{
    build_fix_plan, diagnostic_catalog_json, diagnostic_catalog_text, explain_json, explain_text,
    FixPlanOptions,
};
use crate::effects::{analyze as analyze_effects, effects_to_json};
use crate::format::{format_agent, format_human};
use crate::fuzzing::{run_backend_differential, run_fuzz_suite, BackendDifferentialOptions};
use crate::graph::{
    diff_graphs, dump_graph_text, graph_context_json, graph_context_text, patch_graph,
    roundtrip_graph, validate_graph, view_graph,
};
use crate::inline_tests::{run_inline_tests, InlineTestOptions};
use crate::interp::{run_demo, run_entry_json};
use crate::lexer::lex;
use crate::package::{
    build_package_matrix, create_package, generate_lock, publish_dry_run, run_package_tests,
};
use crate::parser::{parse_file, parse_file_detailed};
use crate::patch::{apply_patch_with_options, diagnose_patch_source, PatchOptions};
use crate::perf::{run_bench, run_perf_report, BenchOptions};
use crate::project::{check_project, generate_project_map_with_options, public_api};
use crate::project_map::{generate_map_with_options, get_symbol, ProjectMapOptions};
use crate::release::release_check;
use crate::security::{audit_project, enforce_package_security, verify_project};
use crate::simd::{
    analyze_simd, check_simd_fallbacks, review_json as simd_review_json,
    review_text as simd_review_text,
};
use crate::span::{diagnostics_to_json as parse_diagnostics_to_json, tokens_to_json, SourceMap};
use crate::stdlib::{standard_library_json, standard_library_text};
use crate::targets::{check_program_target, targets_json, targets_text};
use crate::typecheck::check;
use crate::workbench::{
    context_json, context_text, review_json, review_text as workbench_review_text, ContextOptions,
};

pub fn run(args: Vec<String>) -> Result<(), String> {
    if args.len() < 2 {
        print_help();
        return Ok(());
    }
    match args[1].as_str() {
        "ast" => cmd_ast(&args[2..]),
        "parse" => cmd_parse(&args[2..]),
        "diagnose" => cmd_diagnose(&args[2..]),
        "diagnostics" => cmd_diagnostics(&args[2..]),
        "explain" => cmd_explain(&args[2..]),
        "fmt" => cmd_fmt(&args[2..]),
        "roundtrip" => cmd_roundtrip(&args[2..]),
        "check" => cmd_check(&args[2..]),
        "borrow-report" => cmd_borrow_report(&args[2..]),
        "borrow-conformance" => cmd_borrow_conformance(&args[2..]),
        "cfg" => cmd_cfg(&args[2..]),
        "capsule" => cmd_capsule(&args[2..]),
        "capsule-run" => cmd_capsule_run(&args[2..]),
        "capsule-lock" => cmd_capsule_lock(&args[2..]),
        "capsule-audit" => cmd_capsule_audit(&args[2..]),
        "namespace" => cmd_namespace(&args[2..]),
        "sandbox" => cmd_sandbox(&args[2..]),
        "runtimes" => cmd_runtimes(&args[2..]),
        "native-review" => cmd_native_review(&args[2..]),
        "shell-review" => cmd_shell_review(&args[2..]),
        "contract-review" => cmd_contract_review(&args[2..]),
        "capsule-context" => cmd_capsule_context(&args[2..]),
        "compress" => cmd_compress(&args[2..]),
        "token-report" => cmd_token_report(&args[2..]),
        "edit-bench" => cmd_edit_bench(&args[2..]),
        "capsule-tokens" => cmd_capsule_tokens(&args[2..]),
        "io-run" => cmd_io_run(&args[2..]),
        "net-run" => cmd_net_run(&args[2..]),
        "crypto-selftest" => cmd_crypto_selftest(&args[2..]),
        "polyglot-run" => cmd_polyglot_run(&args[2..]),
        "trust-session" => cmd_trust_session(&args[2..]),
        "asm-check" => cmd_asm_check(&args[2..]),
        "abi" => cmd_abi(&args[2..]),
        "header" => cmd_header(&args[2..]),
        "simd-review" => cmd_simd_review(&args[2..]),
        "backends" => cmd_backends(&args[2..]),
        "ir" => cmd_ir(&args[2..]),
        "stdlib" => cmd_stdlib(&args[2..]),
        "targets" => cmd_targets(&args[2..]),
        "build" => cmd_build(&args[2..]),
        "new" => cmd_new(&args[2..]),
        "lock" => cmd_lock(&args[2..]),
        "test" => cmd_test(&args[2..]),
        "publish" => cmd_publish(&args[2..]),
        "audit" => cmd_audit(&args[2..]),
        "verify" => cmd_verify(&args[2..]),
        "api" => cmd_api(&args[2..]),
        "effects" => cmd_effects(&args[2..]),
        "patch" => cmd_patch(&args[2..]),
        "run" => cmd_run(&args[2..]),
        "map" => cmd_map(&args[2..]),
        "get" => cmd_get(&args[2..]),
        "context" => cmd_context(&args[2..]),
        "review" => cmd_review(&args[2..]),
        "graph" => cmd_graph(&args[2..]),
        "fuzz" => cmd_fuzz(&args[2..]),
        "diff-test" => cmd_diff_test(&args[2..]),
        "bench" => cmd_bench(&args[2..]),
        "perf-report" => cmd_perf_report(&args[2..]),
        "fix" => cmd_fix(&args[2..]),
        "release-check" => cmd_release_check(&args[2..]),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        command => Err(format!("unknown command '{command}'\n\n{}", help_text())),
    }
}

fn cmd_diagnostics(args: &[String]) -> Result<(), String> {
    let subcommand = required_arg(args, 0, "diagnostics list [--json]")?;
    match subcommand {
        "list" => {
            if args.iter().any(|arg| arg == "--json") {
                println!("{}", diagnostic_catalog_json());
            } else {
                print!("{}", diagnostic_catalog_text());
            }
            Ok(())
        }
        other => Err(format!("unknown diagnostics subcommand '{other}'")),
    }
}

fn cmd_explain(args: &[String]) -> Result<(), String> {
    let code = required_arg(args, 0, "explain <diagnostic-code> [--json]")?;
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", explain_json(code)?);
    } else {
        print!("{}", explain_text(code)?);
    }
    Ok(())
}

fn cmd_diagnose(args: &[String]) -> Result<(), String> {
    let file = required_arg(args, 0, "diagnose <file> --json")?;
    let path = Path::new(file);
    let source = fs::read_to_string(path).map_err(|err| format!("failed to read {file}: {err}"))?;
    let json = match path.extension().and_then(|ext| ext.to_str()) {
        Some("fgp") => {
            let lexed = lex(file, &source);
            let mut diagnostics = lexed.diagnostics;
            diagnostics.extend(diagnose_patch_source(file, &source));
            let source_map = SourceMap::new(file, lexed.comments);
            diagnose_json(
                file,
                "fgp",
                None,
                &diagnostics,
                &tokens_to_json(&lexed.tokens),
                &source_map.to_json(),
            )
        }
        _ => {
            let output = parse_file_detailed(path, &source);
            let ast = output
                .program
                .as_ref()
                .map(|program| program.to_canonical_json());
            diagnose_json(
                file,
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .unwrap_or("fgx"),
                ast.as_deref(),
                &output.diagnostics,
                &tokens_to_json(&output.tokens),
                &output.source_map.to_json(),
            )
        }
    };
    if args.iter().any(|arg| arg == "--json") {
        println!("{json}");
    } else {
        println!("diagnose: use --json for machine-readable output");
    }
    Ok(())
}

fn cmd_fix(args: &[String]) -> Result<(), String> {
    if !args.iter().any(|arg| arg == "--plan") {
        return Err(
            "usage: fgx fix --plan <file|project> [--target <triple>] [--json]".to_string(),
        );
    }
    let source = fix_plan_subject(args).ok_or_else(|| {
        "usage: fgx fix --plan <file|project> [--target <triple>] [--json]".to_string()
    })?;
    let report = build_fix_plan(&FixPlanOptions {
        source: source.to_string(),
        target: option_value(args, "--target").map(str::to_string),
    })?;
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    Ok(())
}

fn cmd_ast(args: &[String]) -> Result<(), String> {
    let file = required_arg(args, 0, "ast <file> --canonical")?;
    let program = read_program(file)?;
    if args.iter().any(|arg| arg == "--canonical") {
        println!("{}", program.to_canonical_json());
    } else {
        println!("{}", program.to_json());
    }
    Ok(())
}

fn cmd_parse(args: &[String]) -> Result<(), String> {
    let file = required_arg(args, 0, "parse <file>")?;
    let program = read_program(file)?;
    if args.iter().any(|arg| arg == "--emit-ast") {
        println!("{}", program.to_json());
    } else {
        println!("parsed {} item(s)", program.items.len());
    }
    Ok(())
}

fn cmd_fmt(args: &[String]) -> Result<(), String> {
    let file = required_arg(args, 0, "fmt <file> --to fgx|fgai")?;
    let to = option_value(args, "--to").unwrap_or("fgx");
    let program = read_program(file)?;
    match to {
        "fgx" => print!("{}", format_human(&program)),
        "fgai" => print!("{}", format_agent(&program)),
        other => return Err(format!("unknown format target '{other}'")),
    }
    Ok(())
}

fn cmd_roundtrip(args: &[String]) -> Result<(), String> {
    let file = required_arg(args, 0, "roundtrip <file> --to fgx|fgai")?;
    let to = option_value(args, "--to").unwrap_or("fgai");
    let program = read_program(file)?;
    let rendered = match to {
        "fgx" => format_human(&program),
        "fgai" => format_agent(&program),
        other => return Err(format!("unknown roundtrip target '{other}'")),
    };
    let fake_path = if to == "fgx" {
        PathBuf::from("roundtrip.fgx")
    } else {
        PathBuf::from("roundtrip.fgai")
    };
    let reparsed = parse_file(&fake_path, &rendered)?;
    if reparsed.canonical_eq(&program) {
        println!("roundtrip: ok ({file} -> {to} -> canonical AST)");
    } else {
        println!("roundtrip: changed canonical AST");
    }
    print!("{rendered}");
    Ok(())
}

fn cmd_check(args: &[String]) -> Result<(), String> {
    if args.iter().any(|arg| arg == "--project") {
        let project = project_value(args);
        enforce_package_security(Path::new(project))?;
        let diagnostics = check_project(Path::new(project))?;
        if args.iter().any(|arg| arg == "--json") {
            println!("{}", diagnostics_to_json(&diagnostics));
        } else if diagnostics.is_empty() {
            println!("project check: ok");
        } else {
            for diagnostic in &diagnostics {
                println!(
                    "{} {}: {}",
                    diagnostic.code, diagnostic.node, diagnostic.message
                );
            }
        }
        return Ok(());
    }
    let file = required_arg(args, 0, "check <file> --json")?;
    let program = read_program(file)?;
    let mut diagnostics = check(&program);
    if let Some(triple) = option_value(args, "--target") {
        diagnostics.extend(check_program_target(&program, triple));
        diagnostics.extend(check_simd_fallbacks(&program, triple));
    }
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", diagnostics_to_json(&diagnostics));
    } else if diagnostics.is_empty() {
        println!("check: ok");
    } else {
        for diagnostic in &diagnostics {
            println!(
                "{} {}: {}",
                diagnostic.code, diagnostic.node, diagnostic.message
            );
        }
    }
    Ok(())
}

fn cmd_borrow_report(args: &[String]) -> Result<(), String> {
    let file = required_arg(args, 0, "borrow-report <file> [--json]")?;
    let report = analyze_borrows(Path::new(file))?;
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    if report.is_success() {
        Ok(())
    } else {
        Err("borrow report found diagnostics".to_string())
    }
}

fn cmd_borrow_conformance(args: &[String]) -> Result<(), String> {
    if args.iter().any(|arg| arg != "--json") {
        return Err("usage: fgx borrow-conformance [--json]".to_string());
    }
    let report = run_borrow_conformance();
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    if report.is_success() {
        Ok(())
    } else {
        Err("borrow conformance suite failed".to_string())
    }
}

fn cmd_asm_check(args: &[String]) -> Result<(), String> {
    let file = required_arg(args, 0, "asm-check <file> --target <triple> [--json]")?;
    let triple = option_value(args, "--target")
        .ok_or_else(|| "usage: fgx asm-check <file> --target <triple> [--json]".to_string())?;
    let program = read_program(file)?;
    let result = analyze_assembly(&program, triple);
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", asm_check_json(&result));
    } else if result.diagnostics.is_empty() {
        print!("{}", review_text(&result));
        println!("asm-check: ok");
    } else {
        for diagnostic in &result.diagnostics {
            println!(
                "{} {}: {}",
                diagnostic.code, diagnostic.node, diagnostic.message
            );
        }
    }
    Ok(())
}

fn cmd_abi(args: &[String]) -> Result<(), String> {
    let file = required_arg(args, 0, "abi <file> --target <triple> [--json]")?;
    let triple = option_value(args, "--target")
        .ok_or_else(|| "usage: fgx abi <file> --target <triple> [--json]".to_string())?;
    let program = read_program(file)?;
    let report = analyze_abi(&program, triple);
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", abi_json(&report));
    } else {
        print!("{}", abi_text(&report));
    }
    Ok(())
}

fn cmd_header(args: &[String]) -> Result<(), String> {
    let file = required_arg(args, 0, "header <file> --target <triple>")?;
    let triple = option_value(args, "--target")
        .ok_or_else(|| "usage: fgx header <file> --target <triple>".to_string())?;
    let program = read_program(file)?;
    print!("{}", generate_header(&program, triple)?);
    Ok(())
}

fn cmd_simd_review(args: &[String]) -> Result<(), String> {
    let file = required_arg(args, 0, "simd-review <file> --target <triple> [--json]")?;
    let triple = option_value(args, "--target")
        .ok_or_else(|| "usage: fgx simd-review <file> --target <triple> [--json]".to_string())?;
    let program = read_program(file)?;
    let review = analyze_simd(&program, triple);
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", simd_review_json(&review));
    } else {
        print!("{}", simd_review_text(&review));
        if review.diagnostics.is_empty() {
            println!("simd-review: ok");
        } else {
            for diagnostic in &review.diagnostics {
                println!(
                    "{} {}: {}",
                    diagnostic.code, diagnostic.node, diagnostic.message
                );
            }
        }
    }
    Ok(())
}

fn cmd_targets(args: &[String]) -> Result<(), String> {
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", targets_json());
    } else {
        print!("{}", targets_text());
    }
    Ok(())
}

fn cmd_backends(args: &[String]) -> Result<(), String> {
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", backend_catalog_json());
    } else {
        print!("{}", backend_catalog_text());
    }
    Ok(())
}

fn cmd_stdlib(args: &[String]) -> Result<(), String> {
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", standard_library_json());
    } else {
        print!("{}", standard_library_text());
    }
    Ok(())
}

fn cmd_ir(args: &[String]) -> Result<(), String> {
    let project = required_arg(args, 0, "ir <project> [--json|--contract-json]")?;
    if args.iter().any(|arg| arg == "--contract-json") {
        println!("{}", frontend_contract_json(Path::new(project))?);
        return Ok(());
    }
    let ir = generate_backend_ir(Path::new(project))?;
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", backend_ir_json(&ir));
    } else {
        print!("{}", backend_ir_text(&ir));
    }
    Ok(())
}

fn cmd_api(args: &[String]) -> Result<(), String> {
    if !args.iter().any(|arg| arg == "--public") {
        return Err("usage: fgx api --public [--project <path>]".to_string());
    }
    let project = project_value(args);
    print!("{}", public_api(Path::new(project))?);
    Ok(())
}

fn cmd_build(args: &[String]) -> Result<(), String> {
    let project = required_arg(args, 0, "build <project> --backend c|zig [--out <dir>]")?;
    if args.iter().any(|arg| arg == "--native") {
        return cmd_build_native(project, args);
    }
    let backend = option_value(args, "--backend").unwrap_or("c");
    let default_out = format!("build/{backend}");
    let out_dir = option_value(args, "--out").unwrap_or(&default_out);
    if args.iter().any(|arg| arg == "--all-targets")
        || option_value(args, "--target").is_some()
        || option_value(args, "--profile").is_some()
    {
        let profile = option_value(args, "--profile").unwrap_or("dev");
        let report = build_package_matrix(
            Path::new(project),
            backend,
            Path::new(out_dir),
            profile,
            option_value(args, "--target"),
            args.iter().any(|arg| arg == "--all-targets"),
        )?;
        if args.iter().any(|arg| arg == "--json") {
            println!("{}", report.to_json());
        } else {
            print!("{}", report.to_text());
        }
        return Ok(());
    }
    enforce_package_security(Path::new(project))?;
    let artifacts = build_project_with_backend(Path::new(project), backend, Path::new(out_dir))?;
    for file in artifacts.files {
        println!("generated {}", file.display());
    }
    Ok(())
}

fn cmd_build_native(project: &str, args: &[String]) -> Result<(), String> {
    let kind = match option_value(args, "--emit") {
        Some(value) => crate::native_build::ArtifactKind::parse(value)
            .ok_or_else(|| format!("unknown --emit kind '{value}' (expected obj|shared)"))?,
        None => crate::native_build::ArtifactKind::Object,
    };
    let out_dir = option_value(args, "--out").unwrap_or("build/native");
    enforce_package_security(Path::new(project))?;
    let report = crate::native_build::build(Path::new(project), Path::new(out_dir), kind);
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    if report.is_success() {
        Ok(())
    } else {
        Err(format!("native build failed: {}", report.status))
    }
}

fn cmd_new(args: &[String]) -> Result<(), String> {
    let path = required_arg(args, 0, "new <path> [--name <package>]")?;
    let created = create_package(Path::new(path), option_value(args, "--name"))?;
    println!("created {}", created.manifest.display());
    println!("created {}", created.source.display());
    Ok(())
}

fn cmd_lock(args: &[String]) -> Result<(), String> {
    let project = positional_or_project(args);
    let lock = generate_lock(Path::new(project))?;
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", lock.to_json());
    } else if let Some(out) = option_value(args, "--out") {
        fs::write(out, lock.to_text()).map_err(|err| format!("failed to write {out}: {err}"))?;
        println!("wrote {out}");
    } else {
        print!("{}", lock.to_text());
    }
    Ok(())
}

fn cmd_test(args: &[String]) -> Result<(), String> {
    if args.iter().any(|arg| arg == "--project") {
        let project = project_value(args);
        let report = run_package_tests(
            Path::new(project),
            option_value(args, "--target"),
            args.iter().any(|arg| arg == "--all-targets"),
        )?;
        if args.iter().any(|arg| arg == "--json") {
            println!("{}", report.to_json());
        } else {
            print!("{}", report.to_text());
        }
        if report.is_success() {
            return Ok(());
        }
        return Err("package tests failed".to_string());
    }
    if let Some(file) = args.first().filter(|arg| !arg.starts_with("--")) {
        let report = run_inline_tests(&InlineTestOptions {
            input: PathBuf::from(file),
            target: option_value(args, "--target").map(str::to_string),
            filter: option_value(args, "--filter").map(str::to_string),
        })?;
        if args.iter().any(|arg| arg == "--json") {
            println!("{}", report.to_json());
        } else {
            print!("{}", report.to_text());
        }
        if report.is_success() {
            return Ok(());
        }
        return Err("inline tests failed".to_string());
    }
    let project = project_value(args);
    let report = run_package_tests(
        Path::new(project),
        option_value(args, "--target"),
        args.iter().any(|arg| arg == "--all-targets"),
    )?;
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    if report.is_success() {
        Ok(())
    } else {
        Err("package tests failed".to_string())
    }
}

fn cmd_publish(args: &[String]) -> Result<(), String> {
    if !args.iter().any(|arg| arg == "--dry-run") {
        return Err("usage: fgx publish <project> --dry-run [--json]".to_string());
    }
    let project = positional_or_project(args);
    let report = publish_dry_run(Path::new(project))?;
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    Ok(())
}

fn cmd_audit(args: &[String]) -> Result<(), String> {
    let project = positional_or_project(args);
    let report = audit_project(Path::new(project))?;
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    if report.is_success() {
        Ok(())
    } else {
        Err("supply-chain audit failed".to_string())
    }
}

fn cmd_verify(args: &[String]) -> Result<(), String> {
    let project = positional_or_project(args);
    let report = verify_project(Path::new(project))?;
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    if report.is_success() {
        Ok(())
    } else {
        Err("package verification failed".to_string())
    }
}

fn cmd_capsule_lock(args: &[String]) -> Result<(), String> {
    let file = required_arg(args, 0, "capsule-lock <file> [--json]")?;
    let program = read_program(file)?;
    let report = crate::capsule_lock::lock(file, &program);
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    Ok(())
}

fn cmd_capsule_audit(args: &[String]) -> Result<(), String> {
    let file = required_arg(args, 0, "capsule-audit <file> [--json] [--strict]")?;
    let program = read_program(file)?;
    let report = crate::capsule_lock::audit(file, &program);
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    if args.iter().any(|arg| arg == "--strict") && report.has_high_risk() {
        return Err(format!(
            "capsule supply-chain audit found high risk in {file}"
        ));
    }
    Ok(())
}

fn collect_option_values(args: &[String], flag: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == flag {
            if let Some(value) = args.get(i + 1) {
                out.push(value.clone());
                i += 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn cmd_io_run(args: &[String]) -> Result<(), String> {
    let file = required_arg(
        args,
        0,
        "io-run <file> --entry <fn> [--grant fs.read] [--grant fs.write] [--arg <v>]... [--json]",
    )?;
    let program = read_program(file)?;
    let entry =
        option_value(args, "--entry").ok_or_else(|| "io-run requires --entry <fn>".to_string())?;
    let grants = collect_option_values(args, "--grant");
    let call_args = collect_option_values(args, "--arg");
    let report = crate::io_run::run(file, &program, entry, call_args, grants);
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    if report.is_success() {
        Ok(())
    } else {
        Err(format!("io run: {}", report.status))
    }
}

fn cmd_trust_session(args: &[String]) -> Result<(), String> {
    let capability = option_value(args, "--capability").unwrap_or("optimize.media.h264");
    let payload = option_value(args, "--payload").unwrap_or("patch:add_fast_path");
    let session = crate::trust::run(capability, payload);
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", session.to_json());
    } else {
        print!("{}", session.to_text());
    }
    if session.is_success() {
        Ok(())
    } else {
        Err(format!("trust session: {}", session.status))
    }
}

fn cmd_polyglot_run(args: &[String]) -> Result<(), String> {
    let file = required_arg(args, 0, "polyglot-run <file> [--out <dir>] [--json]")?;
    let program = read_program(file)?;
    let out_dir = option_value(args, "--out").unwrap_or("build/polyglot");
    let report = crate::polyglot::run(file, &program, Path::new(out_dir));
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    if report.is_success() {
        Ok(())
    } else {
        Err(format!("polyglot run: {}", report.status))
    }
}

fn cmd_crypto_selftest(args: &[String]) -> Result<(), String> {
    let report = crate::crypto::self_test();
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    if report.is_success() {
        Ok(())
    } else {
        Err("crypto self-test failed".to_string())
    }
}

fn cmd_net_run(args: &[String]) -> Result<(), String> {
    let file = required_arg(
        args,
        0,
        "net-run <file> --entry <fn> [--allow <host:port>]... [--arg <v>]... [--json]",
    )?;
    let program = read_program(file)?;
    let entry =
        option_value(args, "--entry").ok_or_else(|| "net-run requires --entry <fn>".to_string())?;
    let allow = collect_option_values(args, "--allow");
    let call_args = collect_option_values(args, "--arg");
    let report = crate::net_run::run(file, &program, entry, call_args, allow);
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    if report.is_success() {
        Ok(())
    } else {
        Err(format!("net run: {}", report.status))
    }
}

fn cmd_capsule_tokens(args: &[String]) -> Result<(), String> {
    let file = required_arg(args, 0, "capsule-tokens <file> [--json]")?;
    let program = read_program(file)?;
    let report = crate::capsule_tokens::report(file, &program);
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    Ok(())
}

fn cmd_edit_bench(args: &[String]) -> Result<(), String> {
    let file = required_arg(args, 0, "edit-bench <file> [--edits <n>] [--json]")?;
    let program = read_program(file)?;
    let edits = option_value(args, "--edits")
        .and_then(|n| n.parse::<usize>().ok())
        .unwrap_or(8);
    let report = crate::edit_bench::run(file, &program, edits);
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    Ok(())
}

fn cmd_token_report(args: &[String]) -> Result<(), String> {
    let file = required_arg(args, 0, "token-report <file> [--json]")?;
    let program = read_program(file)?;
    let report = crate::tokens::report(file, &program);
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    Ok(())
}

fn cmd_compress(args: &[String]) -> Result<(), String> {
    let file = required_arg(args, 0, "compress <file> [--emit] [--json]")?;
    let program = read_program(file)?;
    let agent = format_agent(&program);
    let report = crate::compress::compress(&agent);
    if args.iter().any(|arg| arg == "--emit") {
        print!("{}", report.compressed);
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    if !report.roundtrip_ok {
        return Err(format!("compression is not lossless for {file}"));
    }
    Ok(())
}

fn cmd_capsule_context(args: &[String]) -> Result<(), String> {
    let file = required_arg(
        args,
        0,
        "capsule-context <file> [--capsule <name>] [--json]",
    )?;
    let program = read_program(file)?;
    let json_mode = args.iter().any(|arg| arg == "--json");
    match option_value(args, "--capsule") {
        Some(name) => {
            let context = crate::capsule_workbench::context(file, &program, name)?;
            if json_mode {
                println!("{}", context.to_json());
            } else {
                print!("{}", context.to_text());
            }
        }
        None => {
            let inventory = crate::capsule_workbench::inventory(file, &program);
            if json_mode {
                println!("{}", inventory.to_json());
            } else {
                print!("{}", inventory.to_text());
            }
        }
    }
    Ok(())
}

fn cmd_contract_review(args: &[String]) -> Result<(), String> {
    let file = required_arg(args, 0, "contract-review <file> [--json] [--strict]")?;
    let program = read_program(file)?;
    let report = crate::contract::analyze(file, &program);
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    if args.iter().any(|arg| arg == "--strict") && report.is_rejected() {
        return Err(format!("smart-contract capsule review rejected for {file}"));
    }
    Ok(())
}

fn cmd_shell_review(args: &[String]) -> Result<(), String> {
    let file = required_arg(args, 0, "shell-review <file> [--json] [--strict]")?;
    let program = read_program(file)?;
    let report = crate::shell::analyze(file, &program);
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    if args.iter().any(|arg| arg == "--strict") && report.is_rejected() {
        return Err(format!("shell capsule review rejected for {file}"));
    }
    Ok(())
}

fn cmd_native_review(args: &[String]) -> Result<(), String> {
    let file = required_arg(args, 0, "native-review <file> [--json] [--strict]")?;
    let program = read_program(file)?;
    let report = crate::native_capsule::analyze(file, &program);
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    if args.iter().any(|arg| arg == "--strict") && report.is_rejected() {
        return Err(format!("native/GPU capsule review rejected for {file}"));
    }
    Ok(())
}

fn cmd_runtimes(args: &[String]) -> Result<(), String> {
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", crate::runtime::catalog_json());
    } else {
        print!("{}", crate::runtime::catalog_text());
    }
    Ok(())
}

fn cmd_sandbox(args: &[String]) -> Result<(), String> {
    let file = required_arg(args, 0, "sandbox <file> [--json] [--strict]")?;
    let program = read_program(file)?;
    let plan = crate::sandbox::analyze(file, &program);
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", plan.to_json());
    } else {
        print!("{}", plan.to_text());
    }
    if args.iter().any(|arg| arg == "--strict") && plan.is_denied() {
        return Err(format!("sandbox plan denied for {file}"));
    }
    Ok(())
}

fn cmd_namespace(args: &[String]) -> Result<(), String> {
    let file = required_arg(args, 0, "namespace <file> [--json] [--strict]")?;
    let program = read_program(file)?;
    let report = crate::share::analyze(file, &program);
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    if args.iter().any(|arg| arg == "--strict") && report.is_rejected() {
        return Err(format!("shared namespace rejected for {file}"));
    }
    Ok(())
}

fn cmd_capsule_run(args: &[String]) -> Result<(), String> {
    let file = required_arg(
        args,
        0,
        "capsule-run <file> [--capsule <name>] [--input <values>]",
    )?;
    let program = read_program(file)?;
    let name = option_value(args, "--capsule");
    let json_mode = args.iter().any(|arg| arg == "--json");
    // Real C-ABI execution (v2.13): compile, link, and run the capsule.
    if args.iter().any(|arg| arg == "--native") {
        let out_dir = option_value(args, "--out").unwrap_or("build/ffi");
        let plan = crate::ffi::run(file, &program, name, Path::new(out_dir))?;
        if json_mode {
            println!("{}", plan.to_json());
        } else {
            print!("{}", plan.to_text());
        }
        return if plan.is_success() {
            Ok(())
        } else {
            Err(format!("ffi run: {}", plan.execution_status))
        };
    }
    // Real CPython execution (v2.17): run the python capsule against its
    // fixtures on the host interpreter.
    if args.iter().any(|arg| arg == "--python") {
        let report = crate::python_run::run(file, &program, name)?;
        if json_mode {
            println!("{}", report.to_json());
        } else {
            print!("{}", report.to_text());
        }
        return if report.is_success() {
            Ok(())
        } else {
            Err(format!("python run: {}", report.status))
        };
    }
    // Real Node JS/TS execution (v2.20): run the web capsule on host node.
    if args.iter().any(|arg| arg == "--node") {
        let report = crate::node_run::run(file, &program, name)?;
        if json_mode {
            println!("{}", report.to_json());
        } else {
            print!("{}", report.to_text());
        }
        return if report.is_success() {
            Ok(())
        } else {
            Err(format!("node run: {}", report.status))
        };
    }
    // Real JVM execution (v2.22): compile and run the java capsule on host JDK.
    if args.iter().any(|arg| arg == "--java") {
        let report = crate::jvm_run::run(file, &program, name)?;
        if json_mode {
            println!("{}", report.to_json());
        } else {
            print!("{}", report.to_text());
        }
        return if report.is_success() {
            Ok(())
        } else {
            Err(format!("jvm run: {}", report.status))
        };
    }
    // Real .NET execution (v2.23): build and run the csharp capsule on host .NET.
    if args.iter().any(|arg| arg == "--csharp") {
        let report = crate::dotnet_run::run(file, &program, name)?;
        if json_mode {
            println!("{}", report.to_json());
        } else {
            print!("{}", report.to_text());
        }
        return if report.is_success() {
            Ok(())
        } else {
            Err(format!("dotnet run: {}", report.status))
        };
    }
    // Real Go execution (v2.24): assemble and run the go capsule on host Go.
    if args.iter().any(|arg| arg == "--go") {
        let report = crate::go_run::run(file, &program, name)?;
        if json_mode {
            println!("{}", report.to_json());
        } else {
            print!("{}", report.to_text());
        }
        return if report.is_success() {
            Ok(())
        } else {
            Err(format!("go run: {}", report.status))
        };
    }
    // Sandbox-backed execution (v2.3): gate the mock run on sandbox
    // approval for the selected capsule(s).
    if args.iter().any(|arg| arg == "--sandbox") {
        for capsule in program.capsules() {
            if name.map(|n| n == capsule.name).unwrap_or(true)
                && !crate::sandbox::is_approved(capsule)
            {
                return Err(format!(
                    "sandbox denied: capsule '{}' has effects uncovered by its sandbox",
                    capsule.name
                ));
            }
        }
    }
    if let Some(input) = option_value(args, "--input") {
        let values = crate::parser::parse_capsule_values(input)?;
        let outputs = crate::capsule::run_input(&program, name, &values)?;
        if json_mode {
            println!(
                "{{\n  \"schema\": \"forgentex.capsule_run.v1\",\n  \"outputs\": [{}]\n}}",
                outputs
                    .iter()
                    .map(|value| format!(
                        "\"{}\"",
                        value.replace('\\', "\\\\").replace('"', "\\\"")
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        } else {
            println!("outputs: [{}]", outputs.join(", "));
        }
        return Ok(());
    }
    let report = crate::capsule::run(file, &program, name)?;
    if json_mode {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    if args.iter().any(|arg| arg == "--strict") && report.status == "fail" {
        return Err(format!("capsule run failed for {file}"));
    }
    Ok(())
}

fn cmd_capsule(args: &[String]) -> Result<(), String> {
    let file = required_arg(args, 0, "capsule <file> [--json]")?;
    let program = read_program(file)?;
    let report = crate::capsule::analyze(file, &program);
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    if args.iter().any(|arg| arg == "--strict") && report.is_rejected() {
        return Err(format!("capsule review rejected for {file}"));
    }
    Ok(())
}

fn cmd_cfg(args: &[String]) -> Result<(), String> {
    let file = required_arg(args, 0, "cfg <file> [--json]")?;
    let program = read_program(file)?;
    let json_mode = args.iter().any(|arg| arg == "--json");
    if json_mode {
        let mut out = String::from("{\n  \"schema\": \"forgentex.cfg_report.v1\",\n");
        out.push_str(&format!("  \"file\": \"{}\",\n", escape_json(file)));
        out.push_str("  \"functions\": [");
        for (idx, function) in program.functions().enumerate() {
            if idx > 0 {
                out.push(',');
            }
            out.push('\n');
            out.push_str(&cfg_to_json(&build_cfg(function)));
        }
        out.push_str("\n  ]\n}");
        println!("{out}");
    } else {
        for function in program.functions() {
            let cfg = build_cfg(function);
            let carried: Vec<String> = loop_carried_live(&cfg).into_iter().collect();
            println!(
                "{} blocks={} back_edges={} loop_carried=[{}]",
                cfg.function,
                cfg.blocks.len(),
                back_edges(&cfg).len(),
                carried.join(",")
            );
        }
    }
    Ok(())
}

fn cmd_effects(args: &[String]) -> Result<(), String> {
    let file = required_arg(args, 0, "effects <file> --json")?;
    let program = read_program(file)?;
    let effects = analyze_effects(&program);
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", effects_to_json(&effects));
    } else {
        for function in &effects {
            println!(
                "{} declared=[{}] inferred=[{}] missing=[{}]",
                function.id,
                function.declared.join(","),
                function.inferred.join(","),
                function.missing.join(",")
            );
        }
    }
    Ok(())
}

fn cmd_patch(args: &[String]) -> Result<(), String> {
    let source_file = required_arg(
        args,
        0,
        "patch <source> <patch> --review|--check-only|--emit-review-json",
    )?;
    let patch_file = required_arg(
        args,
        1,
        "patch <source> <patch> --review|--check-only|--emit-review-json",
    )?;
    let program = read_program(source_file)?;
    let patch_source = fs::read_to_string(patch_file)
        .map_err(|err| format!("failed to read patch {patch_file}: {err}"))?;
    let check_only = args.iter().any(|arg| arg == "--check-only");
    let emit_review_json = args.iter().any(|arg| arg == "--emit-review-json");
    let result = apply_patch_with_options(program, &patch_source, PatchOptions { check_only })?;
    let diagnostics = check(&result.program);
    if emit_review_json {
        println!("{}", result.review.render_json(&diagnostics));
        return Ok(());
    }
    if check_only || args.iter().any(|arg| arg == "--review") {
        print!("{}", result.review.render());
    }
    if diagnostics.is_empty() {
        println!("Type check after patch: ok");
    } else {
        println!("Type check after patch: diagnostics");
        println!("{}", diagnostics_to_json(&diagnostics));
    }
    if check_only {
        println!("Patch preview only: no patched source emitted.");
        return Ok(());
    }
    println!("\n=== patched.fgx ===");
    print!("{}", format_human(&result.program));
    println!("\n=== patched.fgai ===");
    print!("{}", format_agent(&result.program));
    Ok(())
}

fn cmd_run(args: &[String]) -> Result<(), String> {
    let file = required_arg(args, 0, "run <file>")?;
    let program = read_program(file)?;
    let diagnostics = check(&program);
    if !diagnostics.is_empty() {
        return Err(format!(
            "program has diagnostics and was not run:\n{}",
            diagnostics_to_json(&diagnostics)
        ));
    }
    if let Some(entry) = option_value(args, "--entry") {
        let input = if let Some(input_file) = option_value(args, "--json-input") {
            fs::read_to_string(input_file)
                .map_err(|err| format!("failed to read json input {input_file}: {err}"))?
        } else {
            "{}".to_string()
        };
        println!("{}", run_entry_json(&program, entry, &input)?);
        return Ok(());
    }
    println!("{}", run_demo(&program)?);
    Ok(())
}

fn cmd_map(args: &[String]) -> Result<(), String> {
    let options = ProjectMapOptions {
        include_targets: args.iter().any(|arg| arg == "--include-targets"),
    };
    let map = if args.iter().any(|arg| arg == "--project") {
        generate_project_map_with_options(Path::new(project_value(args)), options)?
    } else {
        let root = required_arg(args, 0, "map <dir> --out <file>")?;
        generate_map_with_options(Path::new(root), options)?
    };
    if let Some(out) = option_value(args, "--out") {
        fs::write(out, &map).map_err(|err| format!("failed to write map {out}: {err}"))?;
        println!("wrote {out}");
    } else {
        print!("{map}");
    }
    Ok(())
}

fn cmd_get(args: &[String]) -> Result<(), String> {
    let target = required_arg(args, 0, "get <symbol> --from <dir> --deps types")?;
    let root = option_value(args, "--from").unwrap_or(".");
    let deps = option_value(args, "--deps");
    print!("{}", get_symbol(Path::new(root), target, deps)?);
    Ok(())
}

fn cmd_context(args: &[String]) -> Result<(), String> {
    let symbol = required_arg(
        args,
        0,
        "context <symbol> --deps types,effects,tests [--json]",
    )?;
    let project = project_value(args);
    let options = ContextOptions::from_csv(option_value(args, "--deps"));
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", context_json(Path::new(project), symbol, options)?);
    } else {
        print!("{}", context_text(Path::new(project), symbol, options)?);
    }
    Ok(())
}

fn cmd_review(args: &[String]) -> Result<(), String> {
    let patch = required_arg(args, 0, "review <patch> [--source <file>] [--json]")?;
    let project = project_value(args);
    let source = option_value(args, "--source").map(Path::new);
    if args.iter().any(|arg| arg == "--json") {
        println!(
            "{}",
            review_json(Path::new(patch), source, Path::new(project))?
        );
    } else {
        print!(
            "{}",
            workbench_review_text(Path::new(patch), source, Path::new(project))?
        );
    }
    Ok(())
}

fn cmd_graph(args: &[String]) -> Result<(), String> {
    let subcommand = required_arg(
        args,
        0,
        "graph <dump|validate|view|patch|diff|roundtrip|context>",
    )?;
    match subcommand {
        "dump" => {
            let input = required_arg(args, 1, "graph dump <project|dir|file> [--out <file>]")?;
            let graph = dump_graph_text(Path::new(input))?;
            if let Some(out) = option_value(args, "--out") {
                fs::write(out, graph)
                    .map_err(|err| format!("failed to write graph {out}: {err}"))?;
                println!("wrote {out}");
            } else {
                print!("{graph}");
            }
            Ok(())
        }
        "validate" => {
            let graph = required_arg(args, 1, "graph validate <graph> [--json]")?;
            let report = validate_graph(Path::new(graph));
            if args.iter().any(|arg| arg == "--json") {
                println!("{}", report.to_json());
            } else {
                print!("{}", report.to_text());
            }
            if report.is_success() {
                Ok(())
            } else {
                Err("graph validation failed".to_string())
            }
        }
        "view" => {
            let graph = required_arg(args, 1, "graph view <graph> --to fgx|fgai")?;
            let to = option_value(args, "--to").unwrap_or("fgx");
            print!("{}", view_graph(Path::new(graph), to)?);
            Ok(())
        }
        "patch" => {
            let graph = required_arg(
                args,
                1,
                "graph patch <graph> <patch> [--out <graph>] [--json]",
            )?;
            let patch = required_arg(
                args,
                2,
                "graph patch <graph> <patch> [--out <graph>] [--json]",
            )?;
            let report = patch_graph(
                Path::new(graph),
                Path::new(patch),
                option_value(args, "--out").map(Path::new),
            )?;
            if args.iter().any(|arg| arg == "--json") {
                println!("{}", report.to_json());
            } else {
                print!("{}", report.to_text());
            }
            if report.is_success() {
                Ok(())
            } else {
                Err("graph patch produced diagnostics".to_string())
            }
        }
        "diff" => {
            let before = required_arg(args, 1, "graph diff <before> <after> [--json]")?;
            let after = required_arg(args, 2, "graph diff <before> <after> [--json]")?;
            let report = diff_graphs(Path::new(before), Path::new(after))?;
            if args.iter().any(|arg| arg == "--json") {
                println!("{}", report.to_json());
            } else {
                print!("{}", report.to_text());
            }
            Ok(())
        }
        "roundtrip" => {
            let input = required_arg(args, 1, "graph roundtrip <project|dir|file> [--json]")?;
            let report = roundtrip_graph(Path::new(input))?;
            if args.iter().any(|arg| arg == "--json") {
                println!("{}", report.to_json());
            } else {
                print!("{}", report.to_text());
            }
            if report.is_success() {
                Ok(())
            } else {
                Err("graph roundtrip failed".to_string())
            }
        }
        "context" => {
            let graph = required_arg(
                args,
                1,
                "graph context <graph> <symbol> --deps types,effects,tests [--json]",
            )?;
            let symbol = required_arg(
                args,
                2,
                "graph context <graph> <symbol> --deps types,effects,tests [--json]",
            )?;
            if args.iter().any(|arg| arg == "--json") {
                println!(
                    "{}",
                    graph_context_json(Path::new(graph), symbol, option_value(args, "--deps"))?
                );
            } else {
                print!(
                    "{}",
                    graph_context_text(Path::new(graph), symbol, option_value(args, "--deps"))?
                );
            }
            Ok(())
        }
        other => Err(format!("unknown graph subcommand '{other}'")),
    }
}

fn cmd_fuzz(args: &[String]) -> Result<(), String> {
    let iterations = option_value(args, "--iterations")
        .unwrap_or("32")
        .parse::<usize>()
        .map_err(|err| format!("invalid --iterations value: {err}"))?;
    let report = run_fuzz_suite(iterations);
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    if report.is_success() {
        Ok(())
    } else {
        Err(format!(
            "fuzz/property suite failed {} case(s)",
            report.failed()
        ))
    }
}

fn cmd_diff_test(args: &[String]) -> Result<(), String> {
    let mut options = BackendDifferentialOptions::default();
    if let Some(project) = option_value(args, "--project") {
        options.project = PathBuf::from(project);
    }
    if let Some(source) = option_value(args, "--source") {
        options.source = PathBuf::from(source);
    }
    if let Some(entry) = option_value(args, "--entry") {
        options.entry = entry.to_string();
    }
    if let Some(input) = option_value(args, "--json-input") {
        options.input = PathBuf::from(input);
    }
    let report = run_backend_differential(&options)?;
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    if report.is_success() {
        Ok(())
    } else {
        Err("backend differential test failed".to_string())
    }
}

fn cmd_bench(args: &[String]) -> Result<(), String> {
    let file = required_arg(
        args,
        0,
        "bench <file> --entry <function> --json-input <file> --target <triple> [--iterations <n>] [--json]",
    )?;
    let entry = option_value(args, "--entry").ok_or_else(|| {
        "usage: fgx bench <file> --entry <function> --json-input <file> --target <triple> [--iterations <n>] [--json]"
            .to_string()
    })?;
    let input = option_value(args, "--json-input").ok_or_else(|| {
        "usage: fgx bench <file> --entry <function> --json-input <file> --target <triple> [--iterations <n>] [--json]"
            .to_string()
    })?;
    let target = option_value(args, "--target").unwrap_or("x86_64-unknown-linux-gnu");
    let iterations = option_value(args, "--iterations")
        .unwrap_or("16")
        .parse::<usize>()
        .map_err(|err| format!("invalid --iterations value: {err}"))?;
    let report = run_bench(&BenchOptions {
        source: PathBuf::from(file),
        entry: entry.to_string(),
        input: PathBuf::from(input),
        target: target.to_string(),
        iterations,
    })?;
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    if report.is_success() {
        Ok(())
    } else {
        Err("benchmark correctness failed".to_string())
    }
}

fn cmd_perf_report(args: &[String]) -> Result<(), String> {
    let scalar = option_value(args, "--scalar").ok_or_else(|| {
        "usage: fgx perf-report --scalar <file> --optimized <file> --entry <function> --optimized-entry <function> --json-input <file> [--iterations <n>] [--json]"
            .to_string()
    })?;
    let optimized = option_value(args, "--optimized").ok_or_else(|| {
        "usage: fgx perf-report --scalar <file> --optimized <file> --entry <function> --optimized-entry <function> --json-input <file> [--iterations <n>] [--json]"
            .to_string()
    })?;
    let entry = option_value(args, "--entry").ok_or_else(|| {
        "usage: fgx perf-report --scalar <file> --optimized <file> --entry <function> --optimized-entry <function> --json-input <file> [--iterations <n>] [--json]"
            .to_string()
    })?;
    let optimized_entry = option_value(args, "--optimized-entry").unwrap_or(entry);
    let input = option_value(args, "--json-input").ok_or_else(|| {
        "usage: fgx perf-report --scalar <file> --optimized <file> --entry <function> --optimized-entry <function> --json-input <file> [--iterations <n>] [--json]"
            .to_string()
    })?;
    let iterations = option_value(args, "--iterations")
        .unwrap_or("16")
        .parse::<usize>()
        .map_err(|err| format!("invalid --iterations value: {err}"))?;
    let report = run_perf_report(
        Path::new(scalar),
        Path::new(optimized),
        entry,
        optimized_entry,
        Path::new(input),
        iterations,
    )?;
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    if report.is_success() {
        Ok(())
    } else {
        Err("performance report gate failed".to_string())
    }
}

fn cmd_release_check(args: &[String]) -> Result<(), String> {
    let target = option_value(args, "--target").unwrap_or("v1.0");
    let report = release_check(target);
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    if args.iter().any(|arg| arg == "--strict") && !report.is_v1_ready() {
        Err(format!("release target '{target}' is not ready"))
    } else {
        Ok(())
    }
}

fn read_program(file: &str) -> Result<crate::ast::Program, String> {
    let path = Path::new(file);
    let source = fs::read_to_string(path).map_err(|err| format!("failed to read {file}: {err}"))?;
    parse_file(path, &source).map_err(|err| format!("failed to parse {file}: {err}"))
}

fn required_arg<'a>(args: &'a [String], index: usize, usage: &str) -> Result<&'a str, String> {
    args.get(index)
        .map(|arg| arg.as_str())
        .ok_or_else(|| format!("usage: fgx {usage}"))
}

fn option_value<'a>(args: &'a [String], option: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0].as_str() == option)
        .map(|pair| pair[1].as_str())
}

fn project_value(args: &[String]) -> &str {
    args.iter()
        .position(|arg| arg == "--project")
        .and_then(|index| args.get(index + 1))
        .filter(|value| !value.starts_with("--"))
        .map(String::as_str)
        .unwrap_or(".")
}

fn positional_or_project(args: &[String]) -> &str {
    args.first()
        .filter(|arg| !arg.starts_with("--"))
        .map(String::as_str)
        .unwrap_or_else(|| project_value(args))
}

fn fix_plan_subject(args: &[String]) -> Option<&str> {
    let mut skip_next = false;
    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        match arg.as_str() {
            "--target" => {
                skip_next = true;
            }
            "--plan" | "--json" => {}
            value if value.starts_with("--") => {}
            value => return Some(value),
        }
    }
    None
}

fn print_help() {
    println!("{}", help_text());
}

fn help_text() -> String {
    "Forgentex demo CLI\n\
\n\
Commands:\n\
  fgx ast <file> --canonical\n\
  fgx parse <file> --emit-ast\n\
  fgx diagnose <file> --json\n\
  fgx diagnostics list [--json]\n\
  fgx explain <diagnostic-code> [--json]\n\
  fgx fmt <file> --to fgx|fgai\n\
  fgx roundtrip <file> --to fgx|fgai\n\
  fgx check <file> --json\n\
  fgx check <file> --target <triple> --json\n\
  fgx borrow-report <file> [--json]\n\
  fgx borrow-conformance [--json]\n\
  fgx cfg <file> [--json]\n\
  fgx capsule <file> [--json] [--strict]\n\
  fgx capsule-run <file> [--capsule <name>] [--input <values>] [--sandbox] [--json] [--strict]\n\
  fgx capsule-run <file> --native [--capsule <name>] [--out <dir>] [--json]\n\
  fgx capsule-lock <file> [--json]\n\
  fgx capsule-audit <file> [--json] [--strict]\n\
  fgx namespace <file> [--json] [--strict]\n\
  fgx sandbox <file> [--json] [--strict]\n\
  fgx runtimes [--json]\n\
  fgx native-review <file> [--json] [--strict]\n\
  fgx shell-review <file> [--json] [--strict]\n\
  fgx contract-review <file> [--json] [--strict]\n\
  fgx capsule-context <file> [--capsule <name>] [--json]\n\
  fgx compress <file> [--emit] [--json]\n\
  fgx token-report <file> [--json]\n\
  fgx build <project> --native [--emit obj|shared] [--out <dir>] [--json]\n\
  fgx edit-bench <file> [--edits <n>] [--json]\n\
  fgx capsule-tokens <file> [--json]\n\
  fgx io-run <file> --entry <fn> [--grant fs.read] [--grant fs.write] [--arg <v>]... [--json]\n\
  fgx net-run <file> --entry <fn> [--allow <host:port>]... [--arg <v>]... [--json]\n\
  fgx crypto-selftest [--json]\n\
  fgx polyglot-run <file> [--out <dir>] [--json]\n\
  fgx trust-session [--capability <cap>] [--payload <p>] [--json]\n\
  fgx asm-check <file> --target <triple> [--json]\n\
  fgx abi <file> --target <triple> [--json]\n\
  fgx header <file> --target <triple>\n\
  fgx simd-review <file> --target <triple> [--json]\n\
  fgx backends [--json]\n\
  fgx ir <project> [--json|--contract-json]\n\
  fgx stdlib [--json]\n\
  fgx check --project [path] --json\n\
  fgx targets [--json]\n\
  fgx build <project> --backend c|zig [--out <dir>]\n\
  fgx build <project> --backend c|zig --target <triple>|--all-targets [--profile dev] [--json]\n\
  fgx new <path> [--name <package>]\n\
  fgx lock [project] [--out <file>] [--json]\n\
  fgx test <file> [--filter <name>] [--target <triple>] [--json]\n\
  fgx test [--project <path>] [--target <triple>|--all-targets] [--json]\n\
  fgx publish <project> --dry-run [--json]\n\
  fgx audit [project] [--json]\n\
  fgx verify [project] [--json]\n\
  fgx api --public [--project <path>]\n\
  fgx effects <file> --json\n\
  fgx patch <source> <patch> --review|--check-only|--emit-review-json\n\
  fgx run <file> [--entry <function> --json-input <file>]\n\
  fgx map <dir> --out <file> [--include-targets]\n\
  fgx map --project [path] --out <file> [--include-targets]\n\
  fgx get <symbol> --from <dir> --deps types\n\
  fgx context <symbol> --deps types,effects,tests [--project <path>] [--json]\n\
  fgx review <patch> [--source <file>] [--project <path>] [--json]\n\
  fgx graph dump <project|dir|file> [--out <file>]\n\
  fgx graph validate <graph> [--json]\n\
  fgx graph view <graph> --to fgx|fgai\n\
  fgx graph patch <graph> <patch> [--out <graph>] [--json]\n\
  fgx graph diff <before> <after> [--json]\n\
  fgx graph roundtrip <project|dir|file> [--json]\n\
  fgx graph context <graph> <symbol> --deps types,effects,tests [--json]\n\
  fgx fuzz [--iterations <n>] [--json]\n\
  fgx diff-test [--project <path>] [--source <file>] --entry <function> --json-input <file> [--json]\n\
  fgx bench <file> --entry <function> --json-input <file> --target <triple> [--iterations <n>] [--json]\n\
  fgx perf-report --scalar <file> --optimized <file> --entry <function> --optimized-entry <function> --json-input <file> [--iterations <n>] [--json]\n\
  fgx fix --plan <file|project> [--target <triple>] [--json]\n\
  fgx release-check [--target v1.0] [--json] [--strict]"
        .to_string()
}

fn diagnose_json(
    file: &str,
    kind: &str,
    canonical_ast: Option<&str>,
    diagnostics: &[crate::span::ParseDiagnostic],
    tokens_json: &str,
    source_map_json: &str,
) -> String {
    let ast = canonical_ast.unwrap_or("null");
    format!(
        "{{\n  \"file\": \"{}\",\n  \"kind\": \"{}\",\n  \"diagnostics\": {},\n  \"tokens\": {},\n  \"source_map\": {},\n  \"canonical_ast\": {}\n}}",
        escape_json(file),
        escape_json(kind),
        parse_diagnostics_to_json(diagnostics),
        tokens_json,
        source_map_json,
        ast
    )
}

use crate::json::escape as escape_json;
