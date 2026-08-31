use std::fs;
use std::path::PathBuf;

use crate::application::{run_dispatch, ApplicationDispatchReport};
use crate::project::load_project;

fn scratch(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "forgentex_{}_pipeline_application_{tag}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("output")).unwrap();
    root
}

fn write_pipeline_application(root: &std::path::Path) {
    fs::write(
        root.join("Forgentex.toml"),
        "[package]\nname = \"pipeline-preview\"\nversion = \"0.1.0\"\nroot = \"src\"\n\n[application]\nforgentex = \"0.1\"\nentry = \"preview\"\nruntime = \"pipeline\"\neffects = [\"alloc\", \"fs.write\"]\n\n[application.limits]\nmax_write_bytes = 1024\n\n[application.artifact.preview]\nroot = \"output\"\nmedia_type = \"application/octet-stream\"\nmax_bytes = 64\nrequired = true\npublication = \"create_new\"\n\n[application.pipeline]\nschedule = \"source\"\ninputs = [\"seed\"]\noutput_share = \"image\"\noutput_path = \"output/preview.bin\"\n\n[application.pipeline.limits]\nmax_stages = 1\nmax_wall_ms = 1000\nmax_cpu_ms = 1000\nmax_memory_bytes = 67108864\nmax_owned_bytes = 64\n\n[permissions]\nfs.write = [\"output\"]\n",
    )
    .unwrap();
    fs::write(
        root.join("src").join("main.fgx"),
        "share seed: i32 = 3\nshare image: Vec[u8]\n\npipeline preview:\n    stage make_preview\n\nforeign make_preview c version \"c11\" effects [alloc]:\n    input value: i32\n    output bytes: Vec[u8]\n    sandbox \"fs.none\"\n    sandbox \"limit.wall_ms 1000\"\n    sandbox \"limit.cpu_ms 1000\"\n    sandbox \"limit.mem_mb 64\"\n    sandbox \"artifact.media_type application/octet-stream\"\n    sandbox \"artifact.max_bytes 64\"\n    bind seed read\n    bind image rw\n    code:\n        #include \"types.h\"\n        #include <stdlib.h>\n\n        FgxOwnedU8 make_preview(int32_t value) {\n            FgxOwnedU8 result = { NULL, 0, 0 };\n            result.ptr = (uint8_t *)malloc(3);\n            if (result.ptr == NULL) return result;\n            result.len = 3;\n            result.capacity = 3;\n            result.ptr[0] = 80;\n            result.ptr[1] = 54;\n            result.ptr[2] = (uint8_t)value;\n            return result;\n        }\n\n        void make_preview_free(uint8_t *ptr, size_t len, size_t capacity) {\n            (void)len;\n            (void)capacity;\n            free(ptr);\n        }\n",
    )
    .unwrap();
}

#[test]
fn pipeline_application_publishes_exact_receipted_bytes_and_refuses_clobber() {
    let root = scratch("publish");
    write_pipeline_application(&root);

    let first = run_dispatch(&root, vec!["9".to_string()]).unwrap();
    let ApplicationDispatchReport::Pipeline(first) = first else {
        panic!("expected pipeline application report");
    };
    assert!(first.is_success(), "{:?}", first.error);
    assert_eq!(
        first.pipeline.graph.as_ref().unwrap().origin,
        "source_declaration"
    );
    assert_eq!(
        first
            .pipeline
            .graph
            .as_ref()
            .unwrap()
            .declaration
            .as_deref(),
        Some("preview")
    );
    assert_eq!(first.artifact.as_ref().unwrap().bytes, 3);
    assert_eq!(
        fs::read(root.join("output/preview.bin")).unwrap(),
        [80, 54, 9]
    );

    let second = run_dispatch(&root, vec!["9".to_string()]).unwrap();
    let ApplicationDispatchReport::Pipeline(second) = second else {
        panic!("expected pipeline application report");
    };
    assert!(!second.is_success());
    assert_eq!(second.error.as_ref().unwrap().code, "io_target_exists");
    assert_eq!(
        fs::read(root.join("output/preview.bin")).unwrap(),
        [80, 54, 9]
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn pipeline_application_manifest_requires_the_bounded_bridge_contract() {
    let root = scratch("manifest");
    fs::write(
        root.join("Forgentex.toml"),
        "[package]\nname = \"bad-pipeline\"\nversion = \"0.1.0\"\nroot = \"src\"\n\n[application]\nforgentex = \"0.1\"\nentry = \"preview\"\nruntime = \"pipeline\"\neffects = [\"fs.write\"]\n\n[application.limits]\nmax_write_bytes = 64\n\n[permissions]\nfs.write = [\"output\"]\n",
    )
    .unwrap();
    fs::write(
        root.join("src/main.fgx"),
        "pipeline preview:\n    stage missing\n",
    )
    .unwrap();

    let error = load_project(&root).unwrap_err();
    assert!(error.contains("requires [application.pipeline]"), "{error}");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn pipeline_application_manifest_requires_every_aggregate_limit() {
    let root = scratch("aggregate_limits");
    write_pipeline_application(&root);
    let manifest = fs::read_to_string(root.join("Forgentex.toml")).unwrap();
    fs::write(
        root.join("Forgentex.toml"),
        manifest.replace("max_cpu_ms = 1000\n", ""),
    )
    .unwrap();

    let error = load_project(&root).unwrap_err();
    assert!(
        error.contains("application.pipeline.limits.max_cpu_ms"),
        "{error}"
    );
    let _ = fs::remove_dir_all(root);
}
