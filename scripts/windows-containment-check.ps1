[CmdletBinding()]
param(
    [string]$Output = "target/windows-containment-evidence.json"
)

$ErrorActionPreference = "Stop"
$repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$targetRoot = [IO.Path]::GetFullPath((Join-Path $repository "target"))
$outputPath = if ([IO.Path]::IsPathRooted($Output)) {
    [IO.Path]::GetFullPath($Output)
} else {
    [IO.Path]::GetFullPath((Join-Path $repository $Output))
}
$targetPrefix = $targetRoot.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
if (!$outputPath.StartsWith($targetPrefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw "evidence output must stay beneath $targetRoot"
}

$tests = @(
    "contained_runner_enforces_the_windows_active_process_limit",
    "contained_runner_reports_windows_peak_process_count",
    "contained_runner_installs_complete_windows_ui_restrictions",
    "windows_native_fs_byte_limit_bounds_aggregate_job_io",
    "windows_live_native_runner_uses_a_kernel_verified_restricted_token",
    "windows_low_integrity_runner_cannot_modify_a_medium_integrity_workspace_file",
    "windows_appcontainer_runner_cannot_read_a_workspace_file",
    "windows_appcontainer_cannot_write_a_user_owned_low_integrity_directory",
    "windows_zero_capability_appcontainer_cannot_connect_to_loopback",
    "windows_zero_capability_appcontainer_cannot_connect_to_host_network",
    "windows_appcontainer_cannot_open_an_unrelated_named_pipe",
    "windows_appcontainer_cannot_open_or_signal_host_named_event",
    "windows_appcontainer_cannot_query_a_hermetic_local_rpc_endpoint",
    "windows_appcontainer_cannot_modify_the_host_user_registry"
)

Push-Location $repository
try {
    $os = Get-CimInstance Win32_OperatingSystem
    $results = foreach ($test in $tests) {
        $timer = [Diagnostics.Stopwatch]::StartNew()
        & cargo test --lib $test -- --nocapture | Out-Host
        $exitCode = $LASTEXITCODE
        $timer.Stop()
        [ordered]@{
            name = $test
            status = if ($exitCode -eq 0) { "pass" } else { "fail" }
            exit_code = $exitCode
            duration_ms = $timer.ElapsedMilliseconds
        }
    }

    $failed = @($results | Where-Object { $_.exit_code -ne 0 })
    $evidence = [ordered]@{
        schema = "forgentex.windows_containment_evidence.v1"
        decision = if ($failed.Count -eq 0) { "pass" } else { "fail" }
        recorded_at_utc = [DateTime]::UtcNow.ToString("o")
        git_commit = (& git rev-parse HEAD).Trim()
        git_status = @(& git status --short --untracked-files=all)
        os = [ordered]@{
            caption = $os.Caption
            version = $os.Version
            build = $os.BuildNumber
            architecture = $os.OSArchitecture
        }
        toolchain = [ordered]@{
            rustc = ((& rustc --version 2>&1 | Select-Object -First 1) -join "")
            cargo = ((& cargo --version 2>&1 | Select-Object -First 1) -join "")
            cc = ((& cc --version 2>&1 | Select-Object -First 1) -join "")
        }
        tests = @($results)
    }

    New-Item -ItemType Directory -Force (Split-Path $outputPath -Parent) | Out-Null
    $evidence | ConvertTo-Json -Depth 6 | Set-Content -Encoding UTF8 $outputPath
    Write-Host "Windows containment evidence: $outputPath"
    Write-Host "decision=$($evidence.decision) tests=$($results.Count) build=$($os.BuildNumber)"
    if ($failed.Count -ne 0) {
        throw "$($failed.Count) Windows containment test(s) failed"
    }
} finally {
    Pop-Location
}
