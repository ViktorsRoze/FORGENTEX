use std::fs;
use std::path::{Path, PathBuf};

use crate::abi::{abi_json, abi_text, analyze_abi, generate_header};
use crate::asmcheck::{analyze_assembly, asm_check_json, review_text};
use crate::backend::{
    backend_by_name, backend_catalog_json, backend_catalog_text, backend_ir_json, backend_ir_text,
    build_project_with_backend, frontend_contract_json, generate_backend_ir,
    run_entry_with_backend, Backend,
};
use crate::borrow::analyze_borrows;
use crate::borrow_conformance::{run_borrow_conformance, run_borrow_conformance_with_campaign};
use crate::cfg::{back_edges, build_cfg, cfg_to_json, loop_carried_live};
use crate::diag::diagnostics_to_json;
use crate::diagnostic_catalog::{
    build_fix_plan, diagnostic_catalog_json, diagnostic_catalog_text, explain_json, explain_text,
    FixPlanOptions,
};
use crate::doctor::doctor_report;
use crate::effects::{analyze as analyze_effects, effects_to_json};
use crate::format::{format_agent, format_human};
use crate::fuzzing::{run_backend_differential, run_fuzz_suite, BackendDifferentialOptions};
use crate::graph::{
    diff_graphs, dump_graph_text, graph_context_json, graph_context_text, patch_graph,
    roundtrip_graph, validate_graph, view_graph,
};
use crate::inline_tests::{run_inline_tests, InlineTestOptions};
use crate::interp::{run_demo_with_ir, run_entry_json_hosted_with_ir};
use crate::lexer::lex;
use crate::package::{
    build_package_matrix, create_package_with_template, generate_lock, publish_dry_run,
    run_package_tests, PackageTemplate,
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
    context_json, context_text, review_json, review_json_with_codebook,
    review_text as workbench_review_text, review_text_with_codebook, ContextOptions,
};

pub fn run(args: Vec<String>) -> Result<(), String> {
    if args.len() < 2 {
        print_help();
        return Ok(());
    }
    match args[1].as_str() {
        "version" | "--version" | "-V" => cmd_version(&args[2..]),
        "doctor" => cmd_doctor(&args[2..]),
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
        "codebook" => cmd_codebook(&args[2..]),
        "token-report" => cmd_token_report(&args[2..]),
        "edit-bench" => cmd_edit_bench(&args[2..]),
        "capsule-tokens" => cmd_capsule_tokens(&args[2..]),
        "app-run" => cmd_app_run(&args[2..]),
        "forgepack" => cmd_forgepack(&args[2..]),
        "io-run" => cmd_io_run(&args[2..]),
        "net-run" => cmd_net_run(&args[2..]),
        "crypto-selftest" => cmd_crypto_selftest(&args[2..]),
        "polyglot-run" => cmd_polyglot_run(&args[2..]),
        "pipeline-run" => cmd_pipeline_run(&args[2..]),
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
        "fs" => cmd_fs(&args[2..]),
        "effects" => cmd_effects(&args[2..]),
        "patch" => cmd_patch(&args[2..]),
        "run" => cmd_run(&args[2..]),
        "debug" => cmd_debug(&args[2..]),
        "dap" => cmd_dap(&args[2..]),
        "optimize" => cmd_optimize(&args[2..]),
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
        "release-evidence" => cmd_release_evidence(&args[2..]),
        "dist-manifest" => cmd_dist_manifest(&args[2..]),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        command => Err(format!("unknown command '{command}'\n\n{}", help_text())),
    }
}

fn cmd_version(args: &[String]) -> Result<(), String> {
    if let Some(argument) = args.first() {
        return Err(format!(
            "unexpected argument '{argument}'\nusage: fgx --version"
        ));
    }
    println!("fgx {}", env!("CARGO_PKG_VERSION"));
    Ok(())
}

fn cmd_doctor(args: &[String]) -> Result<(), String> {
    for argument in args {
        if argument != "--json" && argument != "--strict" {
            return Err(format!(
                "unknown doctor option '{argument}'\nusage: fgx doctor [--json] [--strict]"
            ));
        }
    }
    let report = doctor_report();
    if args.iter().any(|argument| argument == "--json") {
        print!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    if args.iter().any(|argument| argument == "--strict") && !report.is_ready() {
        return Err(format!(
            "doctor status is {}; resolve the reported issues before continuing",
            report.status
        ));
    }
    Ok(())
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
        // A checker that reports errors must not exit successfully. The JSON
        // report is already on stdout, so a wrapper still gets the diagnostics;
        // what changes is that a build script or agent trusting the exit status
        // can no longer mistake a rejected project for a clean one.
        if !diagnostics.is_empty() {
            return Err(format!(
                "project check found {} diagnostic(s)",
                diagnostics.len()
            ));
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
    // A checker that reports errors must not exit successfully. The report is
    // already on stdout, so a wrapper still receives every diagnostic; what
    // changes is that a build script or agent trusting the exit status can no
    // longer mistake a rejected program for a clean one.
    if !diagnostics.is_empty() {
        return Err(format!("check found {} diagnostic(s)", diagnostics.len()));
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
    let usage = "usage: fgx borrow-conformance [--json] [--campaign-seed <u64>] [--campaign-cases <1..256>] [--failure-dir <path>]";
    let mut json = false;
    let mut campaign_seed = crate::borrow_reference::DEFAULT_SAFETY_CAMPAIGN_SEED;
    let mut campaign_cases = crate::borrow_reference::DEFAULT_SAFETY_CAMPAIGN_CASES_PER_FAMILY;
    let mut failure_dir = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--json" => {
                json = true;
                index += 1;
            }
            "--campaign-seed" => {
                let value = args.get(index + 1).ok_or_else(|| usage.to_string())?;
                campaign_seed = parse_u64_seed(value)
                    .ok_or_else(|| format!("invalid campaign seed '{value}'; expected u64"))?;
                index += 2;
            }
            "--campaign-cases" => {
                let value = args.get(index + 1).ok_or_else(|| usage.to_string())?;
                campaign_cases = value.parse::<usize>().map_err(|_| {
                    format!("invalid campaign case count '{value}'; expected 1..256")
                })?;
                if !(1..=256).contains(&campaign_cases) {
                    return Err(format!(
                        "invalid campaign case count '{value}'; expected 1..256"
                    ));
                }
                index += 2;
            }
            "--failure-dir" => {
                failure_dir = Some(
                    args.get(index + 1)
                        .ok_or_else(|| usage.to_string())?
                        .clone(),
                );
                index += 2;
            }
            _ => return Err(usage.to_string()),
        }
    }
    let report = if campaign_seed == crate::borrow_reference::DEFAULT_SAFETY_CAMPAIGN_SEED
        && campaign_cases == crate::borrow_reference::DEFAULT_SAFETY_CAMPAIGN_CASES_PER_FAMILY
    {
        run_borrow_conformance()
    } else {
        run_borrow_conformance_with_campaign(campaign_seed, campaign_cases)
    };
    if let Some(directory) = failure_dir {
        report.persist_campaign_failures(Path::new(&directory))?;
    }
    if json {
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

fn parse_u64_seed(value: &str) -> Option<u64> {
    value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .map_or_else(
            || value.parse().ok(),
            |hex| u64::from_str_radix(hex, 16).ok(),
        )
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
    let project = required_arg(args, 0, "ir <project|file> [--json|--contract-json]")?;
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

/// `fgx fs` — move around the project filesystem carried inside a file.
///
/// The verbs are deliberately the ones an agent already knows from a shell, so
/// finding a payload needs no new vocabulary: `ls` to see what is there, `cat`
/// to read one section, `stat` for its provenance, and `notes` to read the
/// annotation graph. Every section is addressed by its node id, which is the
/// same id a semantic patch targets — so an agent can list, read, and then edit
/// without ever converting between addressing schemes.
fn cmd_fs(args: &[String]) -> Result<(), String> {
    let usage = "fs ls|cat|stat|notes|add|set|rm|note|resolve|changes|extract <file> [--tag <node-id>] [--from <path>] [--out <dir>] [--as data|blob] [--expect <hash>] [--on <node-id>] [--text <text>] [--reply-to <note-id>] [--with-notes] [--against <file>] [--rev <git-rev>] [--since <iso>] [--by <author>] [--at <iso>] [--agent] [--human] [--json]";
    let verb = required_arg(args, 0, usage)?;
    let file = required_arg(args, 1, usage)?;
    let program = read_program(file)?;
    let json = args.iter().any(|arg| arg == "--json");
    let root = Path::new(file).parent().unwrap_or(Path::new("."));
    // Provenance for any verb that writes. `--at` must be the same fixed-width
    // UTC the checker requires, or the file would be written and then rejected.
    let at = || -> Result<String, String> {
        let at = option_value(args, "--at")
            .map(str::to_string)
            .unwrap_or_else(now_iso8601);
        if !crate::section_edit::is_timestamp(&at) {
            return Err(format!("--at '{at}' must be YYYY-MM-DDTHH:MM:SSZ"));
        }
        Ok(at)
    };
    let by = || {
        option_value(args, "--by")
            .map(str::to_string)
            .unwrap_or_else(default_author)
    };
    let expect = option_value(args, "--expect");
    match verb {
        "ls" => {
            if json {
                let entries = program
                    .items
                    .iter()
                    .filter_map(fs_entry_json)
                    .collect::<Vec<_>>();
                println!(
                    "{{\n  \"schema\": \"forgentex.project_fs.v1\",\n  \"file\": {},\n  \"sections\": [{}]\n}}",
                    crate::json::string(file),
                    entries.join(", ")
                );
            } else {
                for item in &program.items {
                    if let Some(line) = fs_entry_text(item) {
                        println!("{line}");
                    }
                }
            }
            Ok(())
        }
        "cat" => {
            let tag = option_value(args, "--tag").ok_or_else(|| format!("usage: {usage}"))?;
            match find_section(&program, tag)? {
                crate::ast::Item::Data(section) => {
                    for line in &section.content {
                        println!("{line}");
                    }
                    Ok(())
                }
                crate::ast::Item::Note(note) => {
                    for line in &note.text {
                        println!("{line}");
                    }
                    Ok(())
                }
                crate::ast::Item::Blob(section) => {
                    // Bytes go to stdout unchanged, so `fgx fs cat --tag
                    // @blob.logo > logo.png` is just a redirect.
                    let bytes = crate::section::blob_bytes(section, root)?;
                    use std::io::Write;
                    std::io::stdout()
                        .write_all(&bytes)
                        .map_err(|err| format!("could not write blob bytes: {err}"))
                }
                other => Err(format!(
                    "'{tag}' is a {} and has no payload to read",
                    crate::ast::item_kind_name(other)
                )),
            }
        }
        "stat" => {
            let tag = option_value(args, "--tag").ok_or_else(|| format!("usage: {usage}"))?;
            let item = find_section(&program, tag)?;
            if json {
                println!(
                    "{}",
                    fs_entry_json(item).unwrap_or_else(|| "null".to_string())
                );
            } else {
                print!("{}", fs_stat_text(item));
            }
            Ok(())
        }
        "notes" => {
            let on = option_value(args, "--on");
            let only_agent = args.iter().any(|arg| arg == "--agent");
            let only_human = args.iter().any(|arg| arg == "--human");
            let selected = program.notes().filter(|note| {
                on.is_none_or(|target| note.target == target)
                    && (!only_agent || note.is_agent())
                    && (!only_human || !note.is_agent())
            });
            if json {
                let rendered = selected
                    .map(|note| {
                        format!(
                            "{{\"id\": {}, \"on\": {}, \"by\": {}, \"agent\": {}, \"status\": {}, \"created\": {}, \"reply_to\": {}, \"text\": {}}}",
                            crate::json::string(&note.id),
                            crate::json::string(&note.target),
                            crate::json::string(&note.author),
                            note.is_agent(),
                            crate::json::string(&note.status),
                            crate::json::opt_string(note.created.as_deref()),
                            crate::json::opt_string(note.reply_to.as_deref()),
                            crate::json::string(&note.text.join("\n"))
                        )
                    })
                    .collect::<Vec<_>>();
                println!(
                    "{{\n  \"schema\": \"forgentex.project_notes.v1\",\n  \"notes\": [{}]\n}}",
                    rendered.join(", ")
                );
            } else {
                for note in selected {
                    let who = if note.is_agent() { "agent" } else { "human" };
                    println!(
                        "{} on {} [{}] {} {}",
                        note.id,
                        note.target,
                        note.status,
                        who,
                        note.created.as_deref().unwrap_or("")
                    );
                    for line in &note.text {
                        println!("    {line}");
                    }
                    if let Some(reply_to) = &note.reply_to {
                        println!("    (reply to {reply_to})");
                    }
                }
            }
            Ok(())
        }
        "add" => {
            let from = option_value(args, "--from").ok_or_else(|| format!("usage: {usage}"))?;
            let source = Path::new(from);
            let name = match option_value(args, "--tag") {
                Some(tag) => tag
                    .trim_start_matches("@data.")
                    .trim_start_matches("@blob.")
                    .to_string(),
                None => source
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .ok_or_else(|| format!("cannot derive a section name from {from}"))?
                    .to_string(),
            };
            let force_blob = matches!(option_value(args, "--as"), Some("blob"));
            let bytes =
                std::fs::read(source).map_err(|err| format!("could not read {from}: {err}"))?;
            let (item, store_write) = crate::section::section_for_payload(
                &name,
                source,
                bytes,
                &at()?,
                &by(),
                force_blob,
            )?;

            let mut program = program;
            let id = crate::ast::item_node_id(&item).to_string();
            if program
                .items
                .iter()
                .any(|existing| crate::ast::item_node_id(existing) == id)
            {
                return Err(format!(
                    "'{id}' is already in this file; remove it first rather than shadowing it"
                ));
            }
            // A payload too large to sit in the file goes to the store first:
            // if that write fails, the project file is left untouched.
            write_store(root, store_write)?;
            program.items.push(item);
            write_project(file, &program)?;
            println!("added {id}");
            Ok(())
        }
        "set" => {
            // Replace a section's payload in place. It keeps its name, shape,
            // and creation record; the edit is stamped as a modification.
            let tag = option_value(args, "--tag").ok_or_else(|| format!("usage: {usage}"))?;
            let from = option_value(args, "--from").ok_or_else(|| format!("usage: {usage}"))?;
            let (name, is_blob) = match find_section(&program, tag)? {
                crate::ast::Item::Data(section) => (section.name.clone(), false),
                crate::ast::Item::Blob(section) => (section.name.clone(), true),
                other => {
                    return Err(format!(
                        "'{tag}' is a {} and has no payload to replace",
                        crate::ast::item_kind_name(other)
                    ))
                }
            };
            crate::section_edit::check_expected(&program, tag, expect)?;
            let bytes =
                std::fs::read(from).map_err(|err| format!("could not read {from}: {err}"))?;
            let (at, by) = (at()?, by());
            // A data section is refilled as text whatever the new file is
            // called: its declared shape belongs to the section, not the file.
            let (item, store_write) = if is_blob {
                crate::section::section_for_payload(&name, Path::new(from), bytes, &at, &by, true)?
            } else {
                if std::str::from_utf8(&bytes).is_err() {
                    return Err(format!(
                        "{from} is not valid UTF-8, so it cannot refill data section '{tag}'"
                    ));
                }
                crate::section::section_for_payload(
                    &name,
                    Path::new("payload.txt"),
                    bytes,
                    &at,
                    &by,
                    false,
                )?
            };
            write_store(root, store_write)?;
            let mut program = program;
            crate::section_edit::replace_payload(&mut program, tag, item, &at, &by)?;
            write_project(file, &program)?;
            println!(
                "set {tag} {}",
                crate::patch::node_hash(&program, tag).unwrap_or_default()
            );
            Ok(())
        }
        "rm" => {
            let tag = option_value(args, "--tag").ok_or_else(|| format!("usage: {usage}"))?;
            crate::section_edit::check_expected(&program, tag, expect)?;
            let with_notes = args.iter().any(|arg| arg == "--with-notes");
            let mut program = program;
            let removed = crate::section_edit::remove_section(&mut program, tag, with_notes)?;
            write_project(file, &program)?;
            for id in removed {
                println!("removed {id}");
            }
            Ok(())
        }
        "note" => {
            let on = option_value(args, "--on").ok_or_else(|| format!("usage: {usage}"))?;
            let text = option_value(args, "--text").ok_or_else(|| format!("usage: {usage}"))?;
            let mut program = program;
            let id = crate::section_edit::add_note(
                &mut program,
                crate::section_edit::NewNote {
                    on,
                    text,
                    reply_to: option_value(args, "--reply-to"),
                    name: option_value(args, "--tag"),
                    status: "open",
                },
                &at()?,
                &by(),
            )?;
            write_project(file, &program)?;
            println!("added {id}");
            Ok(())
        }
        "resolve" => {
            let tag = option_value(args, "--tag").ok_or_else(|| format!("usage: {usage}"))?;
            crate::section_edit::check_expected(&program, tag, expect)?;
            let mut program = program;
            let reply = crate::section_edit::resolve_note(
                &mut program,
                tag,
                option_value(args, "--text"),
                &at()?,
                &by(),
            )?;
            write_project(file, &program)?;
            println!("resolved {tag}");
            println!("added {reply}");
            Ok(())
        }
        "changes" => {
            let changes = if let Some(against) = option_value(args, "--against") {
                crate::section_edit::changes_between(&read_program(against)?, &program)
            } else if let Some(rev) = option_value(args, "--rev") {
                crate::section_edit::changes_between(&program_at_revision(file, rev)?, &program)
            } else if let Some(since) = option_value(args, "--since") {
                crate::section_edit::changes_since(&program, since)?
            } else {
                return Err(format!(
                    "fs changes needs --against <file>, --rev <git-rev>, or --since <iso>; usage: {usage}"
                ));
            };
            if json {
                let entries = changes
                    .iter()
                    .map(|change| {
                        format!(
                            "{{\"id\": {}, \"kind\": {}, \"change\": {}, \"before\": {}, \"after\": {}, \"at\": {}, \"by\": {}}}",
                            crate::json::string(&change.id),
                            crate::json::string(change.kind),
                            crate::json::string(change.change),
                            crate::json::opt_string(change.before.as_deref()),
                            crate::json::opt_string(change.after.as_deref()),
                            crate::json::opt_string(change.at.as_deref()),
                            crate::json::opt_string(change.by.as_deref())
                        )
                    })
                    .collect::<Vec<_>>();
                println!(
                    "{{\n  \"schema\": \"forgentex.project_changes.v1\",\n  \"file\": {},\n  \"changes\": [{}]\n}}",
                    crate::json::string(file),
                    entries.join(", ")
                );
            } else {
                for change in &changes {
                    println!(
                        "{}\t{}\t{}\t{}\t{}",
                        change.change,
                        change.id,
                        change.kind,
                        change.at.as_deref().unwrap_or("-"),
                        change.by.as_deref().unwrap_or("-")
                    );
                }
            }
            Ok(())
        }
        "extract" => {
            let out = option_value(args, "--out").ok_or_else(|| format!("usage: {usage}"))?;
            let out = Path::new(out);
            std::fs::create_dir_all(out)
                .map_err(|err| format!("could not create {}: {err}", out.display()))?;
            let mut written = Vec::new();
            for item in &program.items {
                let Some(name) = crate::section::file_name_for(item) else {
                    continue;
                };
                // Section names are node-safe identifiers, so a name cannot
                // climb out of the output directory; checked anyway, because
                // this writes to a path the caller chose.
                crate::interp::normalize_portable_path(&name)?;
                let bytes = match item {
                    crate::ast::Item::Data(section) => {
                        let mut text = section.content.join("\n");
                        text.push('\n');
                        text.into_bytes()
                    }
                    crate::ast::Item::Blob(section) => crate::section::blob_bytes(section, root)?,
                    _ => continue,
                };
                let target = out.join(&name);
                std::fs::write(&target, &bytes)
                    .map_err(|err| format!("could not write {}: {err}", target.display()))?;
                written.push((
                    crate::ast::item_node_id(item).to_string(),
                    name,
                    bytes.len(),
                ));
            }
            if json {
                let entries = written
                    .iter()
                    .map(|(id, name, bytes)| {
                        format!(
                            "{{\"id\": {}, \"file\": {}, \"bytes\": {bytes}}}",
                            crate::json::string(id),
                            crate::json::string(name)
                        )
                    })
                    .collect::<Vec<_>>();
                println!(
                    "{{\n  \"schema\": \"forgentex.project_fs_extract.v1\",\n  \"out\": {},\n  \"written\": [{}]\n}}",
                    crate::json::string(&out.display().to_string()),
                    entries.join(", ")
                );
            } else {
                for (id, name, bytes) in &written {
                    println!("{id}\t{name}\t{bytes}");
                }
            }
            Ok(())
        }
        other => Err(format!("unknown fs verb '{other}'; usage: {usage}")),
    }
}

/// Put a large payload into the content-addressed store. Identical bytes have
/// an identical path, so an entry that already exists is already right.
fn write_store(root: &Path, store_write: Option<(String, Vec<u8>)>) -> Result<(), String> {
    let Some((relative, bytes)) = store_write else {
        return Ok(());
    };
    let target = root.join(&relative);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("could not create {}: {err}", parent.display()))?;
    }
    if !target.exists() {
        std::fs::write(&target, &bytes)
            .map_err(|err| format!("could not write {}: {err}", target.display()))?;
    }
    Ok(())
}

/// The project as it stood at a git revision, for the change feed.
fn program_at_revision(file: &str, rev: &str) -> Result<crate::ast::Program, String> {
    let path = Path::new(file);
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("cannot name {file} to git"))?;
    // `rev:./name` resolves against the directory git runs in, so the file is
    // found wherever the repository root happens to be.
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .arg("show")
        .arg(format!("{rev}:./{name}"))
        .output()
        .map_err(|err| format!("could not run git: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "git could not produce {file} at {rev}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let text =
        String::from_utf8(output.stdout).map_err(|_| format!("{file} at {rev} is not UTF-8"))?;
    if file.ends_with(".fgai") {
        crate::parser::parse_agent(&text)
    } else {
        crate::parser::parse_human(&text)
    }
    .map_err(|err| format!("{file} at {rev} does not parse: {err}"))
}

/// The node hash of one item, exactly as `NODE_HASH` and `--expect` check it.
fn item_hash(item: &crate::ast::Item) -> String {
    let program = crate::ast::Program {
        items: vec![item.clone()],
    };
    crate::patch::node_hash(&program, crate::ast::item_node_id(item)).unwrap_or_default()
}

/// Write a project back to the file it came from, in the syntax it was written
/// in. A `.fgai` project stays dense; a `.fgx` project stays readable.
fn write_project(file: &str, program: &crate::ast::Program) -> Result<(), String> {
    let rendered = if file.ends_with(".fgai") {
        crate::format::format_agent(program)
    } else {
        crate::format::format_human(program)
    };
    // Parse what will be written before replacing the file: a project that
    // cannot be read back is worse than a failed add.
    let reparsed = if file.ends_with(".fgai") {
        crate::parser::parse_agent(&rendered)
    } else {
        crate::parser::parse_human(&rendered)
    }
    .map_err(|err| format!("refusing to write a file that cannot be parsed back: {err}"))?;
    if reparsed.to_canonical_json() != program.to_canonical_json() {
        return Err(
            "refusing to write: the rendered file does not round-trip to the same program"
                .to_string(),
        );
    }
    std::fs::write(file, rendered).map_err(|err| format!("could not write {file}: {err}"))
}

/// The current UTC time as `YYYY-MM-DDTHH:MM:SSZ`.
///
/// The only clock read in the project. Everything below the command line takes
/// a timestamp as an argument, so parsing, formatting, and checking stay
/// deterministic and testable.
fn now_iso8601() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let time = secs.rem_euclid(86_400);
    // Civil-from-days, so no date library is needed for one timestamp.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        time / 3_600,
        (time % 3_600) / 60,
        time % 60
    )
}

/// Who to record when the caller does not say.
fn default_author() -> String {
    let user = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "unknown".to_string());
    format!("human:{user}")
}

fn find_section<'a>(
    program: &'a crate::ast::Program,
    tag: &str,
) -> Result<&'a crate::ast::Item, String> {
    program
        .items
        .iter()
        .find(|item| crate::ast::item_node_id(item) == tag)
        .ok_or_else(|| format!("no section with node id '{tag}' in this file"))
}

fn fs_entry_text(item: &crate::ast::Item) -> Option<String> {
    match item {
        crate::ast::Item::Data(section) => Some(format!(
            "{}\t{}\tlines={}\t{}",
            section.id,
            section.kind,
            section.content.len(),
            section.meta.modified.as_deref().unwrap_or("-")
        )),
        crate::ast::Item::Blob(section) => Some(format!(
            "{}\t{}\tbytes={}\t{}",
            section.id,
            section.media_type,
            section.bytes,
            section.meta.modified.as_deref().unwrap_or("-")
        )),
        crate::ast::Item::Note(note) => Some(format!(
            "{}\ton {}\t{}\t{}",
            note.id, note.target, note.status, note.author
        )),
        _ => None,
    }
}

fn fs_entry_json(item: &crate::ast::Item) -> Option<String> {
    let meta = |meta: &crate::ast::SectionMeta| {
        format!(
            "\"created\": {}, \"created_by\": {}, \"modified\": {}, \"modified_by\": {}",
            crate::json::opt_string(meta.created.as_deref()),
            crate::json::opt_string(meta.created_by.as_deref()),
            crate::json::opt_string(meta.modified.as_deref()),
            crate::json::opt_string(meta.modified_by.as_deref()),
        )
    };
    match item {
        crate::ast::Item::Data(section) => Some(format!(
            "{{\"id\": {}, \"hash\": {}, \"kind\": \"data\", \"name\": {}, \"shape\": {}, \"lines\": {}, {}}}",
            crate::json::string(&section.id),
            crate::json::string(&item_hash(item)),
            crate::json::string(&section.name),
            crate::json::string(&section.kind),
            section.content.len(),
            meta(&section.meta)
        )),
        crate::ast::Item::Blob(section) => Some(format!(
            "{{\"id\": {}, \"hash\": {}, \"kind\": \"blob\", \"name\": {}, \"media_type\": {}, \"bytes\": {}, \"storage\": {}, \"sha256\": {}, {}}}",
            crate::json::string(&section.id),
            crate::json::string(&item_hash(item)),
            crate::json::string(&section.name),
            crate::json::string(&section.media_type),
            section.bytes,
            crate::json::string(match &section.payload {
                crate::ast::BlobPayload::Inline(_) => "inline",
                crate::ast::BlobPayload::Store(_) => "store",
            }),
            crate::json::opt_string(section.sha256.as_deref()),
            meta(&section.meta)
        )),
        crate::ast::Item::Note(note) => Some(format!(
            "{{\"id\": {}, \"hash\": {}, \"kind\": \"note\", \"on\": {}, \"by\": {}, \"agent\": {}, \"status\": {}, \"created\": {}, \"reply_to\": {}}}",
            crate::json::string(&note.id),
            crate::json::string(&item_hash(item)),
            crate::json::string(&note.target),
            crate::json::string(&note.author),
            note.is_agent(),
            crate::json::string(&note.status),
            crate::json::opt_string(note.created.as_deref()),
            crate::json::opt_string(note.reply_to.as_deref())
        )),
        _ => None,
    }
}

fn fs_stat_text(item: &crate::ast::Item) -> String {
    match item {
        crate::ast::Item::Data(section) => format!(
            "{}\nkind=data shape={} lines={}\ncreated={} by={}\nmodified={} by={}\n",
            section.id,
            section.kind,
            section.content.len(),
            section.meta.created.as_deref().unwrap_or("-"),
            section.meta.created_by.as_deref().unwrap_or("-"),
            section.meta.modified.as_deref().unwrap_or("-"),
            section.meta.modified_by.as_deref().unwrap_or("-")
        ),
        crate::ast::Item::Blob(section) => format!(
            "{}\nkind=blob media_type={} bytes={} storage={}\nsha256={}\ncreated={} by={}\nmodified={} by={}\n",
            section.id,
            section.media_type,
            section.bytes,
            match &section.payload {
                crate::ast::BlobPayload::Inline(_) => "inline",
                crate::ast::BlobPayload::Store(path) => path,
            },
            section.sha256.as_deref().unwrap_or("-"),
            section.meta.created.as_deref().unwrap_or("-"),
            section.meta.created_by.as_deref().unwrap_or("-"),
            section.meta.modified.as_deref().unwrap_or("-"),
            section.meta.modified_by.as_deref().unwrap_or("-")
        ),
        crate::ast::Item::Note(note) => format!(
            "{}\nkind=note on={} by={} agent={} status={}\ncreated={}\nreply_to={}\n",
            note.id,
            note.target,
            note.author,
            note.is_agent(),
            note.status,
            note.created.as_deref().unwrap_or("-"),
            note.reply_to.as_deref().unwrap_or("-")
        ),
        other => format!("{}\n", crate::ast::item_node_id(other)),
    }
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
    // v3.2: `--opt` builds through the optimizer (report + measured C delta).
    if args.iter().any(|arg| arg == "--opt") {
        let (artifacts, report) = crate::backend::build_project_with_backend_opt(
            Path::new(project),
            backend,
            Path::new(out_dir),
        )?;
        print!("{}", report.to_text());
        // Measure the source-size delta against an unoptimized emission.
        let baseline_len = match backend {
            "c" => crate::c_backend::generate_c_for_project(Path::new(project))
                .map(|(_, source)| source.len())
                .ok(),
            "zig" => crate::zig_backend::generate_zig_for_project(Path::new(project))
                .map(|source| source.len())
                .ok(),
            _ => None,
        };
        if let (Some(before), Some(source_file)) = (baseline_len, artifacts.files.last()) {
            if let Ok(after) = fs::metadata(source_file).map(|meta| meta.len() as usize) {
                let saved = before.saturating_sub(after);
                println!("source size: {before} bytes -> {after} bytes ({saved} saved)");
            }
        }
        for file in artifacts.files {
            println!("generated {}", file.display());
        }
        for file in artifacts.metadata_files {
            println!("metadata {}", file.display());
        }
        return Ok(());
    }
    let artifacts = build_project_with_backend(Path::new(project), backend, Path::new(out_dir))?;
    for file in artifacts.files {
        println!("generated {}", file.display());
    }
    for file in artifacts.metadata_files {
        println!("metadata {}", file.display());
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
    let path = required_arg(
        args,
        0,
        "new <path> [--name <package>] [--template lib|app]",
    )?;
    let template = match option_value(args, "--template") {
        Some(value) => PackageTemplate::parse(value)?,
        None => PackageTemplate::default(),
    };
    let created =
        create_package_with_template(Path::new(path), option_value(args, "--name"), template)?;
    println!("created {}", created.manifest.display());
    println!("created {}", created.source.display());
    for extra in &created.extra {
        println!("created {}", extra.display());
    }
    if template == PackageTemplate::App {
        println!("run: fgx app-run {path} --arg \"hello from forgentex\" --json");
    }
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
    let ir = generate_backend_ir(Path::new(file))?;
    let report = crate::io_run::run_with_ir(file, &program, &ir, entry, call_args, grants);
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

fn cmd_app_run(args: &[String]) -> Result<(), String> {
    let project = required_arg(args, 0, "app-run <project> [--arg <v>]... [--json]")?;
    let json = args.iter().any(|arg| arg == "--json");
    let report = match crate::application::run_dispatch(
        Path::new(project),
        collect_option_values(args, "--arg"),
    ) {
        Ok(report) => report,
        Err(error) => {
            if json {
                println!("{}", error.to_json());
                return Err(format!("application run: {}", error.code));
            }
            return Err(error.to_string());
        }
    };
    if json {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    if report.is_success() {
        Ok(())
    } else {
        Err(format!("application run: {}", report.status()))
    }
}

fn cmd_forgepack(args: &[String]) -> Result<(), String> {
    let subcommand = required_arg(
        args,
        0,
        "forgepack build <spec> <output> [--json] | forgepack inspect|verify <archive> [--json] | forgepack extract <archive> <directory> [--json] | forgepack evidence [--json] [--strict]",
    )?;
    let json = args.iter().any(|arg| arg == "--json");
    match subcommand {
        "build" => {
            let spec = required_arg(args, 1, "forgepack build <spec> <output> [--json]")?;
            let output = required_arg(args, 2, "forgepack build <spec> <output> [--json]")?;
            let report = crate::forgepack::build_file(Path::new(spec), output)?;
            if json {
                println!("{}", report.to_json());
            } else {
                print!("{}", report.to_text());
            }
            Ok(())
        }
        "verify" => {
            let archive = required_arg(args, 1, "forgepack verify <archive> [--json]")?;
            let report = crate::forgepack::verify_file(Path::new(archive))?;
            if json {
                println!("{}", report.to_json());
            } else {
                print!("{}", report.to_text());
            }
            Ok(())
        }
        "inspect" => {
            let archive = required_arg(args, 1, "forgepack inspect <archive> [--json]")?;
            let report = crate::forgepack::inspect_file(Path::new(archive))?;
            if json {
                println!("{}", report.to_json());
            } else {
                print!("{}", report.to_text());
            }
            Ok(())
        }
        "extract" => {
            let archive =
                required_arg(args, 1, "forgepack extract <archive> <directory> [--json]")?;
            let directory =
                required_arg(args, 2, "forgepack extract <archive> <directory> [--json]")?;
            let report = crate::forgepack::extract_file(Path::new(archive), Path::new(directory))?;
            if json {
                println!("{}", report.to_json());
            } else {
                print!("{}", report.to_text());
            }
            Ok(())
        }
        "evidence" => {
            let report = crate::forgepack::workflow_evidence();
            if json {
                println!("{}", report.to_json());
            } else {
                print!("{}", report.to_text());
            }
            if args.iter().any(|arg| arg == "--strict") && !report.is_success() {
                Err(format!(
                    "forgepack workflow evidence failed: {}",
                    report.gaps().join("; ")
                ))
            } else {
                Ok(())
            }
        }
        other => Err(format!(
            "unknown forgepack command '{other}'; expected build, inspect, verify, extract, or evidence"
        )),
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

fn cmd_pipeline_run(args: &[String]) -> Result<(), String> {
    let file = required_arg(
        args,
        0,
        "pipeline-run <file> [--set share=value] [--allow-effects a,b] [--schedule declaration|graph|source] [--cache-dir dir] [--cache-key-file file] [--stop-after-stages n] [--max-stages n] [--max-wall-ms n] [--max-cpu-ms n] [--max-memory-bytes n] [--max-owned-bytes n] [--max-fs-bytes n] [--max-net-bytes n] [--json]",
    )?;
    let program = read_program(file)?;
    let mut overrides = Vec::new();
    for item in collect_option_values(args, "--set") {
        let (name, value) = item
            .split_once('=')
            .ok_or_else(|| format!("pipeline --set needs name=value, got '{item}'"))?;
        let mut values = crate::parser::parse_capsule_values(value)?;
        if values.len() != 1 {
            return Err(format!(
                "pipeline --set '{name}' needs exactly one scalar value"
            ));
        }
        overrides.push((name.to_string(), values.remove(0)));
    }
    let allow = option_value(args, "--allow-effects").map(|value| {
        value
            .split(',')
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>()
    });
    let parse_limit = |name: &str| -> Result<Option<u128>, String> {
        option_value(args, name)
            .map(|value| {
                value
                    .parse::<u128>()
                    .ok()
                    .filter(|value| *value > 0)
                    .ok_or_else(|| format!("pipeline {name} needs a positive integer"))
            })
            .transpose()
    };
    let max_stages = parse_limit("--max-stages")?
        .map(|value| {
            usize::try_from(value).map_err(|_| "pipeline --max-stages is too large".to_string())
        })
        .transpose()?;
    let stop_after_stages = parse_limit("--stop-after-stages")?
        .map(|value| {
            usize::try_from(value)
                .map_err(|_| "pipeline --stop-after-stages is too large".to_string())
        })
        .transpose()?;
    let max_owned_bytes = parse_limit("--max-owned-bytes")?
        .map(|value| {
            usize::try_from(value)
                .map_err(|_| "pipeline --max-owned-bytes is too large".to_string())
        })
        .transpose()?;
    let max_memory_bytes = parse_limit("--max-memory-bytes")?
        .map(|value| {
            u64::try_from(value).map_err(|_| "pipeline --max-memory-bytes is too large".to_string())
        })
        .transpose()?;
    let max_fs_bytes = parse_limit("--max-fs-bytes")?
        .map(|value| {
            u64::try_from(value).map_err(|_| "pipeline --max-fs-bytes is too large".to_string())
        })
        .transpose()?;
    let max_net_bytes = parse_limit("--max-net-bytes")?
        .map(|value| {
            u64::try_from(value).map_err(|_| "pipeline --max-net-bytes is too large".to_string())
        })
        .transpose()?;
    let schedule = match option_value(args, "--schedule").unwrap_or("declaration") {
        "declaration" => crate::pipeline::PipelineSchedule::Declaration,
        "graph" => crate::pipeline::PipelineSchedule::DependencyGraph,
        "source" => crate::pipeline::PipelineSchedule::SourceGraph,
        value => {
            return Err(format!(
                "pipeline --schedule '{value}' must be declaration, graph, or source"
            ))
        }
    };
    let limits = crate::pipeline::PipelineLimits {
        max_stages,
        max_wall_ms: parse_limit("--max-wall-ms")?,
        max_cpu_ms: parse_limit("--max-cpu-ms")?,
        max_memory_bytes,
        max_owned_bytes,
        max_fs_bytes,
        max_net_bytes,
    };
    let control = crate::pipeline::PipelineControl {
        cancellation: None,
        stop_after_stages,
    };
    let cache_root = option_value(args, "--cache-dir");
    let cache_key_file = option_value(args, "--cache-key-file");
    if cache_key_file.is_some() && cache_root.is_none() {
        return Err("pipeline --cache-key-file requires --cache-dir".to_string());
    }
    let report = if let Some(root) = cache_root {
        let mut cache = if let Some(key_file) = cache_key_file {
            let mut key =
                read_pipeline_cache_authentication_key(Path::new(root), Path::new(key_file))?;
            let cache =
                crate::pipeline::PipelineCache::authenticated_persistent(Path::new(root), &key);
            key.fill(0);
            cache?
        } else {
            crate::pipeline::PipelineCache::persistent(Path::new(root))?
        };
        crate::pipeline::run_with_cache_control(
            file, &program, overrides, allow, limits, schedule, control, &mut cache,
        )
    } else {
        crate::pipeline::run_with_control(
            file, &program, overrides, allow, limits, schedule, control,
        )
    };
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    if report.is_success() {
        Ok(())
    } else {
        Err(format!("pipeline run: {}", report.status))
    }
}

fn read_pipeline_cache_authentication_key(
    cache_root: &Path,
    key_file: &Path,
) -> Result<Vec<u8>, String> {
    let metadata = std::fs::symlink_metadata(key_file).map_err(|error| {
        format!(
            "pipeline cache authentication key file '{}' is unavailable: {error}",
            key_file.display()
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "pipeline cache authentication key file '{}' must be a non-symlink regular file",
            key_file.display()
        ));
    }
    if !(32..=4096).contains(&metadata.len()) {
        return Err(
            "pipeline cache authentication key file must contain between 32 and 4096 bytes"
                .to_string(),
        );
    }
    let canonical_root = std::fs::canonicalize(cache_root).map_err(|error| {
        format!(
            "pipeline cache directory '{}' cannot be resolved: {error}",
            cache_root.display()
        )
    })?;
    let canonical_key = std::fs::canonicalize(key_file).map_err(|error| {
        format!(
            "pipeline cache authentication key file '{}' cannot be resolved: {error}",
            key_file.display()
        )
    })?;
    if canonical_key.starts_with(&canonical_root) {
        return Err(
            "pipeline cache authentication key file must be outside the cache directory"
                .to_string(),
        );
    }
    let key = std::fs::read(key_file).map_err(|error| {
        format!(
            "pipeline cache authentication key file '{}' cannot be read: {error}",
            key_file.display()
        )
    })?;
    if !(32..=4096).contains(&key.len()) {
        return Err(
            "pipeline cache authentication key file changed while reading or has an invalid size"
                .to_string(),
        );
    }
    Ok(key)
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
    let ir = generate_backend_ir(Path::new(file))?;
    let report = crate::net_run::run_with_ir(file, &program, &ir, entry, call_args, allow);
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
    let report = crate::edit_bench::run(file, &program, edits)?;
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    Ok(())
}

fn cmd_token_report(args: &[String]) -> Result<(), String> {
    let file = required_arg(
        args,
        0,
        "token-report <file> [--bpe-command <program> [--bpe-arg <arg>]... [--bpe-name <label>]] [--json]",
    )?;
    let program = read_program(file)?;
    let mut report = crate::tokens::report(file, &program);
    if let Some(program_path) = option_value(args, "--bpe-command") {
        let extra: Vec<&str> = args
            .windows(2)
            .filter(|pair| pair[0] == "--bpe-arg")
            .map(|pair| pair[1].as_str())
            .collect();
        let label = option_value(args, "--bpe-name")
            .map(str::to_string)
            .unwrap_or_else(|| {
                std::iter::once(program_path)
                    .chain(extra.iter().copied())
                    .collect::<Vec<_>>()
                    .join(" ")
            });
        report.measured = Some(crate::tokens::measure(&program, &label, |text| {
            count_with_external_tokenizer(program_path, &extra, text)
        })?);
    }
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    Ok(())
}

/// Count one text's tokens with an external tokenizer.
///
/// The protocol is deliberately small so any tokenizer can sit behind it: the
/// text arrives on stdin exactly as Forgentex would send it, and the program
/// prints a single non-negative integer. `tools/tokenizer/tiktoken_count.py` is
/// one such program. Anything else on stdout is an error rather than something
/// to parse around, because a count read out of noise is worse than none.
fn count_with_external_tokenizer(
    program: &str,
    args: &[&str],
    text: &str,
) -> Result<usize, String> {
    use std::io::Write;
    let mut child = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|err| format!("could not start tokenizer '{program}': {err}"))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "tokenizer stdin was not piped".to_string())?;
    let payload = text.as_bytes().to_vec();
    // Written from a thread so a tokenizer that prints before it finishes
    // reading cannot deadlock against a full pipe.
    let writer = std::thread::spawn(move || stdin.write_all(&payload));
    let output = child
        .wait_with_output()
        .map_err(|err| format!("tokenizer '{program}' failed: {err}"))?;
    writer
        .join()
        .map_err(|_| "tokenizer input writer panicked".to_string())?
        .map_err(|err| format!("could not send text to tokenizer '{program}': {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "tokenizer '{program}' exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let printed = String::from_utf8_lossy(&output.stdout);
    printed.trim().parse::<usize>().map_err(|_| {
        format!(
            "tokenizer '{program}' must print one non-negative integer, got {:?}",
            printed.trim()
        )
    })
}

fn cmd_codebook(args: &[String]) -> Result<(), String> {
    let subcommand = required_arg(
        args,
        0,
        "codebook build|check|measure|stats|encode|decode [project|input] [--codebook <file>] [--previous <file>] [--out <file>] [--budget <n>] [--cryptic] [--json]",
    )?;
    let json = args.iter().any(|arg| arg == "--json");
    let project = args
        .get(1)
        .filter(|value| !value.starts_with("--"))
        .map(String::as_str)
        .unwrap_or(".");
    match subcommand {
        "build" => {
            let out = option_value(args, "--out").unwrap_or("forgentex.codebook.json");
            let budget = option_value(args, "--budget")
                .map(|value| {
                    value
                        .parse::<usize>()
                        .map_err(|_| "codebook --budget needs a positive integer".to_string())
                })
                .transpose()?
                .unwrap_or(crate::codebook::DEFAULT_CODEBOOK_BUDGET);
            let previous = option_value(args, "--previous")
                .map(|path| crate::codebook::read(Path::new(path)))
                .transpose()?;
            let report = crate::codebook::build_with_previous(
                Path::new(project),
                budget,
                args.iter().any(|arg| arg == "--cryptic"),
                previous.as_ref(),
            )?;
            fs::write(out, report.to_json())
                .map_err(|error| format!("failed to write codebook {out}: {error}"))?;
            if json {
                println!("{}", report.to_json());
            } else {
                print!("{}", report.to_text());
                println!("wrote {out}");
            }
            Ok(())
        }
        "check" => {
            let path = option_value(args, "--codebook").unwrap_or("forgentex.codebook.json");
            let report = crate::codebook::check(Path::new(project), Path::new(path))?;
            if json {
                println!("{}", report.to_json());
            } else {
                print!("{}", report.to_text());
            }
            if report.is_success() {
                Ok(())
            } else {
                Err("codebook check: stale".to_string())
            }
        }
        "measure" => {
            let path = option_value(args, "--codebook").unwrap_or("forgentex.codebook.json");
            let codebook = crate::codebook::read(Path::new(path))?;
            let report = crate::codebook::measure(Path::new(project), &codebook)?;
            if json {
                println!("{}", report.to_json());
            } else {
                print!("{}", report.to_text());
            }
            Ok(())
        }
        "stats" => {
            let path = option_value(args, "--codebook").unwrap_or("forgentex.codebook.json");
            let report = crate::codebook::read(Path::new(path))?;
            if json {
                println!("{}", report.to_json());
            } else {
                print!("{}", report.stats_text());
            }
            Ok(())
        }
        "encode" | "decode" => {
            let input = required_arg(
                args,
                1,
                "codebook encode|decode <input> --codebook <file> --out <file>",
            )?;
            let codebook_path = option_value(args, "--codebook")
                .ok_or_else(|| "codebook encode/decode requires --codebook <file>".to_string())?;
            let out = option_value(args, "--out")
                .ok_or_else(|| "codebook encode/decode requires --out <file>".to_string())?;
            let codebook = crate::codebook::read(Path::new(codebook_path))?;
            let source = fs::read_to_string(input)
                .map_err(|error| format!("failed to read codebook payload {input}: {error}"))?;
            let (transformed, notice) = if subcommand == "encode" {
                let admission = if args.iter().any(|arg| arg == "--force-codebook") {
                    let encoded = codebook.encode_payload(&source)?;
                    crate::codebook::CodebookPayload {
                        plain_tokens: crate::tokens::estimate(&source),
                        encoded_tokens: crate::tokens::estimate(&encoded),
                        text: encoded,
                        decision: "forced".to_string(),
                    }
                } else {
                    codebook.encode_payload_if_smaller(&source)?
                };
                let notice = format!(
                    "codebook={} plain_tokens={} encoded_tokens={}",
                    admission.decision, admission.plain_tokens, admission.encoded_tokens
                );
                (admission.text, notice)
            } else {
                (
                    codebook.decode_payload(&source)?,
                    "codebook=decoded".to_string(),
                )
            };
            fs::write(out, transformed)
                .map_err(|error| format!("failed to write codebook payload {out}: {error}"))?;
            println!("wrote {out} {notice}");
            Ok(())
        }
        other => Err(format!(
            "unknown codebook command '{other}': expected build, check, measure, stats, encode, or decode"
        )),
    }
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

/// The tracing debugger (v0): a deterministic step trace whose node ids are
/// the same canonical ids semantic patches target. See docs/debugger_plan.md.
/// v3.2: the dependency-free optimizer. Reports the passes applied
/// (forgentex.optimize.v1); `--emit` prints the optimized human source.
/// In-memory only — source files are never rewritten.
fn cmd_optimize(args: &[String]) -> Result<(), String> {
    let file = required_arg(args, 0, "optimize <file> [--json] [--emit]")?;
    let mut program = read_program(file)?;
    let report = crate::optimize::optimize_program(&mut program);
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    if args.iter().any(|arg| arg == "--emit") {
        print!("{}", crate::format::format_human(&program));
    }
    Ok(())
}

fn cmd_debug(args: &[String]) -> Result<(), String> {
    let file = required_arg(
        args,
        0,
        "debug <file> --entry <fn> [--json-input <in>] [--break <node-id>] [--max-steps N] [--live] [--allow-effects a,b] [--grant-fs a,b] [--grant-net host,...] [--values summary|full] [--tail N] [--break-if <node>=<expr>] [--watch <expr>] [--interactive] [--json]",
    )?;
    let program = read_program(file)?;
    let diagnostics = check(&program);
    if !diagnostics.is_empty() {
        return Err(format!(
            "program has diagnostics and was not run:
{}",
            diagnostics_to_json(&diagnostics)
        ));
    }
    let entry =
        option_value(args, "--entry").ok_or_else(|| "debug requires --entry <fn>".to_string())?;
    let input = if let Some(input_file) = option_value(args, "--json-input") {
        fs::read_to_string(input_file)
            .map_err(|err| format!("failed to read json input {input_file}: {err}"))?
    } else {
        "{}".to_string()
    };
    let mut options = crate::interp::DebugOptions {
        break_at: option_value(args, "--break").map(str::to_string),
        ..Default::default()
    };
    if let Some(max) = option_value(args, "--max-steps") {
        options.max_steps = max
            .parse::<usize>()
            .map_err(|_| format!("invalid --max-steps value: {max}"))?;
    }
    // Runtime reach mirrors `fgx run`: `--live` executes capsules, `--hosted`
    // plus grants opens scoped filesystem and network access. Both stay off by
    // default so an ordinary debug run cannot touch the host.
    let split = |value: &str| {
        value
            .split(',')
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>()
    };
    options.live = args.iter().any(|arg| arg == "--live");
    options.allow_effects = option_value(args, "--allow-effects").map(split);
    if options.allow_effects.is_some() {
        options.live = true;
    }
    options.fs_grants = option_value(args, "--grant-fs")
        .map(split)
        .unwrap_or_default();
    options.net_allow = option_value(args, "--grant-net")
        .map(split)
        .unwrap_or_default();
    options.hosted = !options.fs_grants.is_empty() || !options.net_allow.is_empty();
    // Traced values are elided by default; a debug run over a multi-megabyte
    // buffer otherwise renders the whole buffer on every step.
    if let Some(tail) = option_value(args, "--tail") {
        options.tail = Some(
            tail.parse::<usize>()
                .map_err(|_| format!("invalid --tail value: {tail}"))?,
        );
    }
    match option_value(args, "--values") {
        Some("full") => options.value_elements = usize::MAX,
        Some("summary") | None => {}
        Some(other) => {
            return Err(format!(
                "invalid --values value '{other}': expected summary or full"
            ))
        }
    }

    // `--break-if <node>=<expr>` stops at a node only when the expression holds
    // there; `--watch <expr>` is repeatable and evaluates after every step.
    if let Some(spec) = option_value(args, "--break-if") {
        let (node, source) = spec
            .split_once('=')
            .ok_or_else(|| format!("debug --break-if needs <node-id>=<expr>, got '{spec}'"))?;
        let expr = crate::parser::parse_expr(source)
            .map_err(|err| format!("invalid --break-if expression '{source}': {err}"))?;
        options.break_if = Some((node.to_string(), expr));
    }
    for source in collect_option_values(args, "--watch") {
        let expr = crate::parser::parse_expr(&source)
            .map_err(|err| format!("invalid --watch expression '{source}': {err}"))?;
        options.watches.push((source, expr));
    }

    if args.iter().any(|arg| arg == "--interactive") {
        return run_interactive_debug(&program, Path::new(file), entry, &input, options);
    }

    let mut report = crate::interp::debug_entry(&program, entry, &input, options)?;
    // Node ids are the tracer's vocabulary; line numbers are resolved from the
    // source map afterwards so a step can be opened in an editor.
    let path = Path::new(file);
    if let Ok(source) = fs::read_to_string(path) {
        let parsed = crate::parser::parse_file_detailed(path, &source);
        report.resolve_positions(&parsed.source_map);
    }
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    Ok(())
}

/// Drive a paused debug session from stdin.
///
/// One command per line: `step`/`s` into calls, `next`/`n` over them,
/// `out`/`o` until the frame returns, `continue`/`c`, `print <expr>`/`p`, and
/// `quit`/`q`. End-of-input continues to completion rather than hanging.
fn run_interactive_debug(
    program: &crate::ast::Program,
    file: &Path,
    entry: &str,
    input: &str,
    options: crate::interp::DebugOptions,
) -> Result<(), String> {
    use crate::interp::{DebugCommand, DebugEvent};
    use std::io::Write;

    let source_map = fs::read_to_string(file)
        .ok()
        .map(|source| crate::parser::parse_file_detailed(file, &source).source_map);
    let line_of = |node: &str| {
        source_map.as_ref().and_then(|map| {
            map.nodes
                .iter()
                .find(|entry| entry.node == node)
                .map(|entry| entry.span.start_line)
        })
    };

    println!("forgentex debug session: step | next | out | continue | print <expr> | quit");
    let stdin = std::io::stdin();
    let mut report = None;
    // The last stop is kept so `locals` can answer after an `print`, which
    // leaves the session paused at the same place.
    let mut last_locals: Vec<(String, String)> = Vec::new();

    // Read one command. `print` returns an Evaluate, which leaves the session
    // paused and produces an Evaluated event that prompts again.
    let prompt = |locals: &[(String, String)]| -> DebugCommand {
        loop {
            print!("(fgx) ");
            let _ = std::io::stdout().flush();
            let mut line = String::new();
            // End of input is not an error: finish the run.
            if stdin.read_line(&mut line).unwrap_or(0) == 0 {
                return DebugCommand::Continue;
            }
            let line = line.trim();
            let (head, rest) = line.split_once(' ').unwrap_or((line, ""));
            match head {
                "step" | "s" => return DebugCommand::StepIn,
                "next" | "n" => return DebugCommand::Next,
                "out" | "o" => return DebugCommand::StepOut,
                "continue" | "c" | "" => return DebugCommand::Continue,
                "quit" | "q" => return DebugCommand::Quit,
                "locals" | "l" => {
                    for (name, value) in locals {
                        println!("  {name} = {value}");
                    }
                }
                "print" | "p" if !rest.trim().is_empty() => {
                    return DebugCommand::Evaluate(rest.trim().to_string())
                }
                "print" | "p" => println!("  print needs an expression"),
                other => println!("  unknown command '{other}'"),
            }
        }
    };

    crate::interp::debug_session(program, entry, input, options, |event| match event {
        DebugEvent::Stopped(stop) => {
            let position = line_of(&stop.node)
                .map(|line| format!("L{line}"))
                .unwrap_or_else(|| "--".to_string());
            println!(
                "stopped {position} {} (depth {})  {}",
                stop.node, stop.depth, stop.text
            );
            last_locals = stop.locals.clone();
            prompt(&stop.locals)
        }
        DebugEvent::Evaluated { source, value } => {
            match value {
                Ok(text) => println!("  {source} = {text}"),
                Err(err) => println!("  {source}: {err}"),
            }
            // Still paused at the same place: ask again.
            prompt(&last_locals)
        }
        DebugEvent::Finished(finished) => {
            report = Some(finished.clone());
            DebugCommand::Continue
        }
    })?;

    if let Some(report) = report {
        println!("session {}", report.status);
    }
    Ok(())
}

/// Serve one Debug Adapter Protocol session on stdio, for an editor to drive.
fn cmd_dap(_args: &[String]) -> Result<(), String> {
    let stdin = std::io::stdin();
    let mut reader = std::io::BufReader::new(stdin.lock());
    let stdout = std::io::stdout();
    let mut writer = std::io::BufWriter::new(stdout.lock());
    crate::dap::serve(&mut reader, &mut writer)
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
    // Julia and MoonBit capsules: fixtures run on the host Julia, or built to
    // WebAssembly and run in moonrun's deny-all sandbox.
    for (flag, language) in [("--julia", "julia"), ("--moonbit", "moonbit")] {
        if args.iter().any(|arg| arg == flag) {
            let report = if language == "julia" {
                crate::julia_run::run(file, &program, name)?
            } else {
                crate::moonbit_run::run(file, &program, name)?
            };
            if json_mode {
                println!("{}", report.to_json());
            } else {
                print!("{}", report.to_text());
            }
            return if report.is_success() {
                Ok(())
            } else {
                Err(format!("{language} run: {}", report.status))
            };
        }
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
        "patch <source> <patch> --review|--check-only|--emit-review-json [--codebook <file>] [--project <path>]",
    )?;
    let patch_file = required_arg(
        args,
        1,
        "patch <source> <patch> --review|--check-only|--emit-review-json [--codebook <file>] [--project <path>]",
    )?;
    let program = read_program(source_file)?;
    let mut patch_source = fs::read_to_string(patch_file)
        .map_err(|err| format!("failed to read patch {patch_file}: {err}"))?;
    if let Some(path) = option_value(args, "--codebook") {
        let codebook = crate::codebook::read(Path::new(path))?;
        codebook.ensure_current(Path::new(project_value(args)))?;
        patch_source = codebook.decode_payload(&patch_source)?;
    }
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
    let mut program = read_program(file)?;
    let diagnostics = check(&program);
    if !diagnostics.is_empty() {
        return Err(format!(
            "program has diagnostics and was not run:\n{}",
            diagnostics_to_json(&diagnostics)
        ));
    }
    // v3.2: `--opt` runs the dependency-free optimizer on the checked program
    // (in memory only) before executing.
    if args.iter().any(|arg| arg == "--opt") {
        let _ = crate::optimize::optimize_program(&mut program);
    }
    if let Some(entry) = option_value(args, "--entry") {
        let input = if let Some(input_file) = option_value(args, "--json-input") {
            fs::read_to_string(input_file)
                .map_err(|err| format!("failed to read json input {input_file}: {err}"))?
        } else {
            "{}".to_string()
        };
        let backend = option_value(args, "--backend").unwrap_or("interp");
        // `--live` enables the hosted program runtime (rt.1 + dom.1): capsule
        // calls reach real host runtimes and fs/net builtins run against the
        // granted scopes, each domain gated and audited. Off by default so a
        // plain run stays deterministic and touches nothing. `--allow-effects
        // a,b` restricts capsule effects; `--grant-fs fs.read,fs.write` grants
        // filesystem capabilities; `--grant-net host,...` allowlists endpoints.
        if args.iter().any(|arg| arg == "--live") {
            let backend_kind =
                backend_by_name(backend).ok_or_else(|| format!("unknown backend '{backend}'"))?;
            if backend_kind.info().name != "interp" {
                return Err(format!(
                    "backend '{}' does not support hosted live runs yet; use --backend interp",
                    backend_kind.info().name
                ));
            }
            let split = |value: &str| {
                value
                    .split(',')
                    .map(|item| item.trim().to_string())
                    .filter(|item| !item.is_empty())
                    .collect::<Vec<_>>()
            };
            let allow = option_value(args, "--allow-effects").map(split);
            let fs_grants = option_value(args, "--grant-fs")
                .map(split)
                .unwrap_or_default();
            let net_allow = option_value(args, "--grant-net")
                .map(split)
                .unwrap_or_default();
            // rt.3: warm workers (one persistent process per capsule) are the
            // DEFAULT for live runs — measured 8.6-9.8x on 10-call loops, with
            // auto-respawn on worker death. `--cold` restores per-call spawn;
            // `--warm` is accepted for compatibility (it is the default).
            let warm = !args.iter().any(|arg| arg == "--cold");
            // `--trusted` is the operator's explicit acceptance that managed
            // capsules may run even when they declare limits the runtime cannot
            // enforce; those are waived and named in the call's receipt. Every
            // enforceable limit still applies. A capsule can never grant this.
            let policy = crate::interp::CapsulePolicy {
                warm,
                managed: crate::managed_exec::ManagedPolicy {
                    trusted: args.iter().any(|arg| arg == "--trusted"),
                },
            };
            let ir = generate_backend_ir(Path::new(file))?;
            println!(
                "{}",
                run_entry_json_hosted_with_ir(
                    &program, &ir, entry, &input, allow, fs_grants, net_allow, policy
                )?
            );
        } else {
            println!(
                "{}",
                run_entry_with_backend(&program, backend, entry, &input)?.json
            );
        }
        return Ok(());
    }
    let ir = generate_backend_ir(Path::new(file))?;
    println!("{}", run_demo_with_ir(&program, &ir)?);
    Ok(())
}

fn cmd_map(args: &[String]) -> Result<(), String> {
    let options = ProjectMapOptions {
        include_targets: args.iter().any(|arg| arg == "--include-targets"),
    };
    let project_mode = args.iter().any(|arg| arg == "--project");
    let mut codebook_notice = None;
    let mut map = if project_mode {
        generate_project_map_with_options(Path::new(project_value(args)), options)?
    } else {
        let root = required_arg(args, 0, "map <dir> --out <file>")?;
        generate_map_with_options(Path::new(root), options)?
    };
    if let Some(path) = option_value(args, "--codebook") {
        if !project_mode {
            return Err(
                "map --codebook requires --project so corpus currency can be verified".to_string(),
            );
        }
        let codebook = crate::codebook::read(Path::new(path))?;
        codebook.ensure_current(Path::new(project_value(args)))?;
        let admission = if args.iter().any(|arg| arg == "--force-codebook") {
            let encoded = codebook.encode_payload(&map)?;
            crate::codebook::CodebookPayload {
                plain_tokens: crate::tokens::estimate(&map),
                encoded_tokens: crate::tokens::estimate(&encoded),
                text: encoded,
                decision: "forced".to_string(),
            }
        } else {
            codebook.encode_payload_if_smaller(&map)?
        };
        codebook_notice = Some(format!(
            "codebook={} plain_tokens={} encoded_tokens={}",
            admission.decision, admission.plain_tokens, admission.encoded_tokens
        ));
        map = admission.text;
    }
    if let Some(out) = option_value(args, "--out") {
        fs::write(out, &map).map_err(|err| format!("failed to write map {out}: {err}"))?;
        if let Some(notice) = codebook_notice {
            println!("wrote {out} {notice}");
        } else {
            println!("wrote {out}");
        }
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
        "context <symbol> --deps types,effects,tests [--max-tokens <n>] [--since <context.json>] [--codebook <file>] [--force-codebook] [--out <context.json>] [--json]",
    )?;
    let project = project_value(args);
    let max_tokens = option_value(args, "--max-tokens")
        .map(|value| {
            value
                .parse::<usize>()
                .ok()
                .filter(|value| *value > 0)
                .ok_or_else(|| "context --max-tokens needs a positive integer".to_string())
        })
        .transpose()?;
    let mut options =
        ContextOptions::from_csv(option_value(args, "--deps")).with_max_tokens(max_tokens);
    if let Some(path) = option_value(args, "--codebook") {
        options = options
            .with_codebook(crate::codebook::read(Path::new(path))?)
            .with_force_codebook(args.iter().any(|arg| arg == "--force-codebook"));
    }
    if let Some(path) = option_value(args, "--since") {
        let baseline = fs::read_to_string(path)
            .map_err(|error| format!("failed to read context baseline {path}: {error}"))?;
        options = options.with_baseline_json(&baseline)?;
    }
    if let Some(path) = option_value(args, "--out") {
        let json = context_json(Path::new(project), symbol, options)?;
        fs::write(path, json)
            .map_err(|error| format!("failed to write context packet {path}: {error}"))?;
        println!("wrote {path}");
    } else if args.iter().any(|arg| arg == "--json") {
        println!("{}", context_json(Path::new(project), symbol, options)?);
    } else {
        print!("{}", context_text(Path::new(project), symbol, options)?);
    }
    Ok(())
}

fn cmd_review(args: &[String]) -> Result<(), String> {
    let patch = required_arg(
        args,
        0,
        "review <patch> [--source <file>] [--project <path>] [--codebook <file>] [--json]",
    )?;
    let project = project_value(args);
    let source = option_value(args, "--source").map(Path::new);
    let codebook = option_value(args, "--codebook")
        .map(|path| crate::codebook::read(Path::new(path)))
        .transpose()?;
    if args.iter().any(|arg| arg == "--json") {
        println!(
            "{}",
            if codebook.is_some() {
                review_json_with_codebook(
                    Path::new(patch),
                    source,
                    Path::new(project),
                    codebook.as_ref(),
                )?
            } else {
                review_json(Path::new(patch), source, Path::new(project))?
            }
        );
    } else {
        print!(
            "{}",
            if codebook.is_some() {
                review_text_with_codebook(
                    Path::new(patch),
                    source,
                    Path::new(project),
                    codebook.as_ref(),
                )?
            } else {
                workbench_review_text(Path::new(patch), source, Path::new(project))?
            }
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
    if args.iter().any(|arg| arg == "--strict") && !report.is_target_ready() {
        Err(format!("release target '{target}' is not ready"))
    } else {
        Ok(())
    }
}

fn cmd_release_evidence(args: &[String]) -> Result<(), String> {
    let report = crate::public_compat::public_0_1_evidence();
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    if args.iter().any(|arg| arg == "--strict") && report.status != "pass" {
        Err("public 0.1 evidence has unresolved gaps".to_string())
    } else {
        Ok(())
    }
}

fn cmd_dist_manifest(args: &[String]) -> Result<(), String> {
    let manifest = crate::distribution::distribution_manifest();
    if args.iter().any(|arg| arg == "--json") {
        println!("{}", manifest.to_json());
    } else {
        print!("{}", manifest.to_text());
    }
    Ok(())
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
    "Forgentex CLI\n\
\n\
Commands:\n\
  fgx --version\n\
  fgx doctor [--json] [--strict]\n\
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
  fgx borrow-conformance [--json] [--campaign-seed <u64>] [--campaign-cases <1..256>] [--failure-dir <path>]\n\
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
  fgx codebook build|check|measure|stats|encode|decode [project|input] [--codebook <file>] [--previous <file>] [--out <file>] [--budget <n>] [--cryptic] [--force-codebook] [--json]\n\
  fgx token-report <file> [--json]\n\
  fgx build <project> --native [--emit obj|shared] [--out <dir>] [--json]\n\
  fgx edit-bench <file> [--edits <n>] [--json]\n\
  fgx capsule-tokens <file> [--json]\n\
  fgx app-run <project> [--arg <v>]... [--json]\n\
  fgx forgepack build <spec> <output> [--json]\n\
  fgx forgepack evidence [--json] [--strict]\n\
  fgx forgepack extract <archive> <directory> [--json]\n\
  fgx forgepack inspect <archive> [--json]\n\
  fgx forgepack verify <archive> [--json]\n\
  fgx io-run <file> --entry <fn> [--grant fs.read] [--grant fs.write] [--arg <v>]... [--json]\n\
  fgx net-run <file> --entry <fn> [--allow <host:port>]... [--arg <v>]... [--json]\n\
  fgx crypto-selftest [--json]\n\
  fgx polyglot-run <file> [--out <dir>] [--json]\n\
  fgx pipeline-run <file> [--set share=value] [--allow-effects a,b] [--schedule declaration|graph|source] [--cache-dir dir] [--cache-key-file file] [--stop-after-stages n] [--max-stages n] [--max-wall-ms n] [--max-cpu-ms n] [--max-memory-bytes n] [--max-owned-bytes n] [--max-fs-bytes n] [--max-net-bytes n] [--json]\n\
  fgx trust-session [--capability <cap>] [--payload <p>] [--json]\n\
  fgx asm-check <file> --target <triple> [--json]\n\
  fgx abi <file> --target <triple> [--json]\n\
  fgx header <file> --target <triple>\n\
  fgx simd-review <file> --target <triple> [--json]\n\
  fgx backends [--json]\n\
  fgx ir <project|file> [--json|--contract-json]\n\
  fgx stdlib [--json]\n\
  fgx check --project [path] --json\n\
  fgx targets [--json]\n\
  fgx build <project> --backend c|zig [--out <dir>]\n\
  fgx build <project> --backend c|zig --target <triple>|--all-targets [--profile dev] [--json]\n\
  fgx new <path> [--name <package>] [--template lib|app]\n\
  fgx lock [project] [--out <file>] [--json]\n\
  fgx test <file> [--filter <name>] [--target <triple>] [--json]\n\
  fgx test [--project <path>] [--target <triple>|--all-targets] [--json]\n\
  fgx publish <project> --dry-run [--json]\n\
  fgx audit [project] [--json]\n\
  fgx verify [project] [--json]\n\
  fgx api --public [--project <path>]\n\
  fgx fs ls|stat <file> [--tag <node-id>] [--json]\n\
  fgx fs cat <file> --tag <node-id>\n\
  fgx fs notes <file> [--on <node-id>] [--agent] [--human] [--json]\n\
  fgx fs add <file> --from <path> [--tag <name>] [--as data|blob] [--by <author>] [--at <iso>]\n\
  fgx fs set <file> --tag <node-id> --from <path> [--expect <hash>] [--by <author>] [--at <iso>]\n\
  fgx fs rm <file> --tag <node-id> [--expect <hash>] [--with-notes]\n\
  fgx fs note <file> --on <node-id> --text <text> [--reply-to <note-id>] [--tag <name>] [--by <author>] [--at <iso>]\n\
  fgx fs resolve <file> --tag <note-id> [--text <reason>] [--expect <hash>] [--by <author>] [--at <iso>]\n\
  fgx fs changes <file> --rev <git-rev>|--against <file>|--since <iso> [--json]\n\
  fgx fs extract <file> --out <dir> [--json]\n\
  fgx effects <file> --json\n\
  fgx patch <source> <patch> --review|--check-only|--emit-review-json [--codebook <file>] [--project <path>]\n\
  fgx run <file> [--entry <function> --json-input <file> --backend interp]\n\
  fgx run <file> --entry <function> --json-input <file> --live [--allow-effects a,b] [--grant-fs a,b] [--grant-net host,...] [--cold] [--trusted]\n\
  fgx dap
\n  fgx debug <file> --entry <fn> [--json-input <in>] [--break <node-id>] [--max-steps N] [--live] [--allow-effects a,b] [--grant-fs a,b] [--grant-net host,...] [--values summary|full] [--tail N] [--break-if <node>=<expr>] [--watch <expr>] [--interactive] [--json]\n\
  fgx map <dir> --out <file> [--include-targets]\n\
  fgx map --project [path] --out <file> [--include-targets] [--codebook <file>] [--force-codebook]\n\
  fgx get <symbol> --from <dir> --deps types\n\
  fgx context <symbol> --deps types,effects,tests [--max-tokens <n>] [--since <context.json>] [--codebook <file>] [--force-codebook] [--out <context.json>] [--project <path>] [--json]\n\
  fgx review <patch> [--source <file>] [--project <path>] [--codebook <file>] [--json]\n\
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
  fgx release-check [--target v1.0|0.1] [--json] [--strict]\n\
  fgx release-evidence [--json] [--strict]\n\
  fgx dist-manifest [--json]"
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
        "{{\n  \"schema\": \"{}\",\n  \"file\": \"{}\",\n  \"kind\": \"{}\",\n  \"diagnostics\": {},\n  \"tokens\": {},\n  \"source_map\": {},\n  \"canonical_ast\": {}\n}}",
        crate::public_compat::DIAGNOSE_SCHEMA,
        escape_json(file),
        escape_json(kind),
        parse_diagnostics_to_json(diagnostics),
        tokens_json,
        source_map_json,
        ast
    )
}

use crate::json::escape as escape_json;
