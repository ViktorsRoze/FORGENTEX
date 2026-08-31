use std::fs;
use std::path::Path;

const EXTENSION_ROOT: &str = "tools/vscode/forgentex";

#[test]
fn vscode_extension_package_declares_languages_commands_and_grammars() {
    let package = read("package.json");
    for language in [
        "forgentex-fgx",
        "forgentex-fgai",
        "forgentex-fgp",
        "forgentex-fgmap",
    ] {
        assert!(package.contains(language), "missing language {language}");
    }
    for command in [
        "forgentex.showHumanAgentViews",
        "forgentex.contextForSymbol",
        "forgentex.reviewPatch",
        "forgentex.applyPatchPreview",
        "forgentex.runDiagnostics",
        "forgentex.showUnsafeArchitectureSlices",
    ] {
        assert!(package.contains(command), "missing command {command}");
    }
    for grammar in [
        "syntaxes/fgx.tmLanguage.json",
        "syntaxes/fgai.tmLanguage.json",
        "syntaxes/fgp.tmLanguage.json",
        "syntaxes/fgmap.tmLanguage.json",
    ] {
        assert!(package.contains(grammar), "missing grammar {grammar}");
    }
}

#[test]
fn vscode_grammars_cover_patches_node_ids_unsafe_and_architectures() {
    let fgx = read("syntaxes/fgx.tmLanguage.json");
    // Assert the node-ID alternation covers each kind rather than matching one
    // exact string, so widening it to new declaration kinds is not a failure.
    for kind in ["function", "struct", "module", "statement", "expression"] {
        assert!(
            fgx.contains(&format!("{kind}|")) || fgx.contains(&format!("|{kind})")),
            "node-id pattern does not cover {kind}"
        );
    }
    assert!(fgx.contains("unsafe"));
    assert!(fgx.contains("asm|ASM"));
    assert!(fgx.contains("x86_64"));
    assert!(fgx.contains("aarch64"));
    assert!(fgx.contains("riscv64"));

    let fgp = read("syntaxes/fgp.tmLanguage.json");
    assert!(fgp.contains("TARGET"));
    assert!(fgp.contains("OP"));
    assert!(fgp.contains("ADD_ARCH_VARIANT"));
    assert!(fgp.contains("LOWER_TO_IR"));
}

#[test]
fn vscode_extension_bridges_to_fgx_cli_language_service() {
    let extension = read("extension.js");
    for required in [
        "fgxPath",
        "\"fmt\"",
        "\"context\"",
        "\"review\"",
        "\"patch\"",
        "\"check\"",
        "createWebviewPanel",
        "registerHoverProvider",
        "registerDocumentSymbolProvider",
        "showUnsafeArchitectureSlices",
    ] {
        assert!(
            extension.contains(required),
            "missing extension bridge token {required}"
        );
    }
}

#[test]
fn vscode_extension_documentation_is_present() {
    let docs =
        fs::read_to_string("docs/vscode_extension.md").expect("missing docs/vscode_extension.md");
    assert!(docs.contains("Stage 20"));
    assert!(docs.contains("ARM/AArch64"));
    assert!(docs.contains("RISC-V"));

    let readme = read("README.md");
    assert!(readme.contains("Patch review WebView"));
    assert!(readme.contains("architecture filters"));
}

fn read(relative: &str) -> String {
    let path = Path::new(EXTENSION_ROOT).join(relative);
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()))
}
