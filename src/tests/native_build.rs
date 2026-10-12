use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use crate::native_build::{build, run_with_path_contained_limited, ArtifactKind, ProcessLimits};
#[cfg(windows)]
use crate::native_build::{
    run_with_path_timeout_limited, DEFAULT_CONTAINED_PROCESS_LIMIT, WINDOWS_JOB_UI_RESTRICTIONS,
};

use super::manifest_path;

fn out_dir(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("forgentex_{}_native_{tag}", std::process::id()))
}

#[test]
#[cfg(windows)]
fn windows_appcontainer_profile_lookup_is_parallel_safe() {
    let workers = 16;
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(workers));
    let handles = (0..workers)
        .map(|_| {
            let barrier = std::sync::Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                (0..4)
                    .map(|_| crate::native_build::windows_native_appcontainer_sid_string())
                    .collect::<Result<Vec<_>, _>>()
            })
        })
        .collect::<Vec<_>>();

    let mut expected: Option<String> = None;
    for handle in handles {
        for sid in handle
            .join()
            .expect("profile lookup thread panicked")
            .unwrap()
        {
            assert!(!sid.is_empty());
            if let Some(expected) = &expected {
                assert_eq!(&sid, expected);
            } else {
                expected = Some(sid);
            }
        }
    }
}

#[test]
#[cfg(windows)]
fn windows_appcontainer_profile_policy_is_versioned_and_never_deletes_at_runtime() {
    assert!(crate::native_build::NATIVE_APPCONTAINER_NAME.ends_with(".0_1"));
    assert_eq!(
        crate::native_build::NATIVE_APPCONTAINER_PROFILE_POLICY,
        "versioned_immutable_no_runtime_delete"
    );
    assert!(
        !include_str!("../native_build.rs").contains("DeleteAppContainerProfile"),
        "runtime containment must never delete an AppContainer profile"
    );
}

#[test]
fn native_build_collects_facts_and_honors_exact() {
    let out = out_dir("facts");
    let _ = fs::remove_dir_all(&out);
    let report = build(
        &manifest_path("examples/native"),
        &out,
        ArtifactKind::Object,
    );

    assert_eq!(report.schema, "forgentex.native_build.v1");
    assert_eq!(report.functions, 4);
    for name in ["clamp_i32", "mix_avg_i32", "scale_q15"] {
        assert!(
            report.exact_functions.iter().any(|f| f == name),
            "expected @exact function {name}, got {:?}",
            report.exact_functions
        );
    }
    assert!(!report.c_source_hash.is_empty());

    // Either the host compiled a real artifact, or no compiler is present.
    // Both are a Forgentex success.
    assert!(report.is_success(), "unexpected status {}", report.status);
    if report.status == "built" {
        let path = report.artifact_path.as_ref().expect("artifact path");
        assert!(
            std::path::Path::new(path).exists(),
            "artifact missing: {path}"
        );
        assert!(report.artifact_bytes.unwrap_or(0) > 0);
        assert!(report.toolchain.is_some());
    }
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn native_build_c_source_hash_is_deterministic() {
    let a = build(
        &manifest_path("examples/native"),
        &out_dir("d1"),
        ArtifactKind::Object,
    );
    let b = build(
        &manifest_path("examples/native"),
        &out_dir("d2"),
        ArtifactKind::Object,
    );
    assert_eq!(a.c_source_hash, b.c_source_hash);
    assert_eq!(a.c_header_hash, b.c_header_hash);
    let _ = fs::remove_dir_all(out_dir("d1"));
    let _ = fs::remove_dir_all(out_dir("d2"));
}

#[test]
fn native_build_json_is_well_formed() {
    let report = build(
        &manifest_path("examples/native"),
        &out_dir("json"),
        ArtifactKind::Object,
    );
    let json = report.to_json();
    assert!(json.contains("\"schema\": \"forgentex.native_build.v1\""));
    assert!(json.contains("\"exact_functions\""));
    assert!(json.contains("\"c_source_hash\""));
    let _ = fs::remove_dir_all(out_dir("json"));
}

#[test]
fn artifact_kind_parses_known_forms() {
    assert_eq!(ArtifactKind::parse("obj"), Some(ArtifactKind::Object));
    assert_eq!(ArtifactKind::parse("object"), Some(ArtifactKind::Object));
    assert_eq!(ArtifactKind::parse("shared"), Some(ArtifactKind::Shared));
    assert!(ArtifactKind::parse("exe").is_none());
}

#[test]
#[cfg(any(unix, windows))]
fn contained_runner_does_not_leave_a_background_descendant() {
    let out = out_dir("process_tree");
    let _ = fs::remove_dir_all(&out);
    fs::create_dir_all(&out).unwrap();
    let started = out.join("started.txt");
    let escaped = out.join("escaped.txt");

    #[cfg(windows)]
    let argv = {
        let quote = |path: &std::path::Path| path.display().to_string().replace('\'', "''");
        let child = out.join("child.ps1");
        let parent = out.join("parent.ps1");
        fs::write(
            &child,
            format!(
                "[IO.File]::WriteAllText('{}', 'started')\nStart-Sleep -Milliseconds 500\n[IO.File]::WriteAllText('{}', 'escaped')\n",
                quote(&started),
                quote(&escaped)
            ),
        )
        .unwrap();
        fs::write(
            &parent,
            format!(
                "$child = '{}'\nStart-Process -FilePath 'powershell.exe' -ArgumentList @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $child) -WindowStyle Hidden\nwhile (!(Test-Path -LiteralPath '{}')) {{ Start-Sleep -Milliseconds 10 }}\n",
                quote(&child),
                quote(&started)
            ),
        )
        .unwrap();
        vec![
            "powershell.exe".to_string(),
            "-NoProfile".to_string(),
            "-ExecutionPolicy".to_string(),
            "Bypass".to_string(),
            "-File".to_string(),
            parent.display().to_string(),
        ]
    };

    #[cfg(unix)]
    let argv = {
        let child = out.join("child.sh");
        let parent = out.join("parent.sh");
        fs::write(
            &child,
            format!(
                "printf started > '{}'\nsleep 0.5\nprintf escaped > '{}'\n",
                started.display(),
                escaped.display()
            ),
        )
        .unwrap();
        fs::write(
            &parent,
            format!(
                "sh '{}' &\nwhile [ ! -f '{}' ]; do sleep 0.01; done\n",
                child.display(),
                started.display()
            ),
        )
        .unwrap();
        vec!["sh".to_string(), parent.display().to_string()]
    };

    let output = run_with_path_contained_limited(&argv, None, ProcessLimits::default()).unwrap();
    assert!(output.status.success(), "output: {output:?}");
    assert!(started.exists(), "descendant did not start");
    std::thread::sleep(Duration::from_millis(800));
    assert!(
        !escaped.exists(),
        "background descendant escaped process-tree containment"
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
#[cfg(any(unix, windows))]
fn contained_runner_rejects_combined_output_over_budget() {
    #[cfg(windows)]
    let argv = vec![
        "powershell.exe".to_string(),
        "-NoProfile".to_string(),
        "-Command".to_string(),
        "[Console]::Out.Write('x' * 4096)".to_string(),
    ];

    #[cfg(unix)]
    let argv = vec![
        "sh".to_string(),
        "-c".to_string(),
        "i=0; while [ $i -lt 4096 ]; do printf x; i=$((i + 1)); done".to_string(),
    ];

    let limits = ProcessLimits {
        output_bytes: 1024,
        ..ProcessLimits::default()
    };
    let error = run_with_path_contained_limited(&argv, None, limits).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::FileTooLarge);
    assert!(error
        .to_string()
        .contains("combined 1024-byte output limit"));
}

#[test]
#[cfg(windows)]
fn contained_runner_enforces_the_windows_active_process_limit() {
    let out = out_dir("process_limit");
    let _ = fs::remove_dir_all(&out);
    fs::create_dir_all(&out).unwrap();
    let child = out.join("child.ps1");
    let parent = out.join("parent.ps1");
    fs::write(&child, "Start-Sleep -Seconds 5\n").unwrap();
    let escaped_child = child.display().to_string().replace('\'', "''");
    fs::write(
        &parent,
        format!(
            "$ErrorActionPreference = 'Stop'\n1..4 | ForEach-Object {{ Start-Process -FilePath 'powershell.exe' -ArgumentList @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', '{escaped_child}') -WindowStyle Hidden | Out-Null }}\n"
        ),
    )
    .unwrap();
    let argv = vec![
        "powershell.exe".to_string(),
        "-NoProfile".to_string(),
        "-ExecutionPolicy".to_string(),
        "Bypass".to_string(),
        "-File".to_string(),
        parent.display().to_string(),
    ];
    let limits = ProcessLimits {
        active_processes: Some(2),
        ..ProcessLimits::default()
    };
    let output = run_with_path_contained_limited(&argv, None, limits).unwrap();
    assert!(
        !output.status.success(),
        "process fan-out unexpectedly exceeded the two-process Job Object limit"
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
#[cfg(windows)]
fn contained_runner_reports_windows_peak_process_count() {
    let argv = vec![
        "powershell.exe".to_string(),
        "-NoProfile".to_string(),
        "-Command".to_string(),
        "Start-Sleep -Milliseconds 100".to_string(),
    ];
    let run = run_with_path_timeout_limited(
        &argv,
        None,
        Duration::from_secs(2),
        ProcessLimits::default(),
    )
    .unwrap()
    .expect("contained process should finish");
    assert!(run.output.status.success(), "output: {:?}", run.output);
    let peak = run
        .peak_tree_processes
        .expect("Windows Job Object must report active processes");
    assert!((1..=DEFAULT_CONTAINED_PROCESS_LIMIT).contains(&peak));
}

#[test]
#[cfg(windows)]
fn contained_runner_installs_complete_windows_ui_restrictions() {
    let argv = vec![
        "powershell.exe".to_string(),
        "-NoProfile".to_string(),
        "-Command".to_string(),
        "Start-Sleep -Milliseconds 50".to_string(),
    ];
    let run = run_with_path_timeout_limited(
        &argv,
        None,
        Duration::from_secs(2),
        ProcessLimits::default(),
    )
    .unwrap()
    .expect("contained process should finish");
    assert!(run.output.status.success(), "output: {:?}", run.output);
    assert_eq!(
        run.windows_ui_restrictions,
        Some(WINDOWS_JOB_UI_RESTRICTIONS)
    );
}
