$ErrorActionPreference = "Stop"

$CargoCommand = Get-Command cargo -ErrorAction SilentlyContinue
if ($CargoCommand) {
    $Cargo = $CargoCommand.Source
} else {
    $Cargo = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
}

& $Cargo fmt --check
& $Cargo clippy -- -D warnings
& $Cargo test
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\fuzz.ps1
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\demo.ps1
