$ErrorActionPreference = "Stop"

$CargoCommand = Get-Command cargo -ErrorAction SilentlyContinue
if ($CargoCommand) {
    $Cargo = $CargoCommand.Source
} else {
    $Cargo = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
}

& $Cargo run --bin fgx -- fuzz --iterations 32 --json
& $Cargo run --bin fgx -- diff-test --json
