param(
    [string]$FgxPath = "target/release/fgx.exe"
)

$resolved = (Resolve-Path -LiteralPath $FgxPath).Path
$installRoot = Join-Path ([IO.Path]::GetTempPath()) "forgentex-forgepack-clean-install-$PID"
if (Test-Path -LiteralPath $installRoot) {
    throw "clean-install target already exists: $installRoot"
}

New-Item -ItemType Directory -Path $installRoot | Out-Null
try {
    $installed = Join-Path $installRoot ([IO.Path]::GetFileName($resolved))
    Copy-Item -LiteralPath $resolved -Destination $installed
    & $installed forgepack evidence --strict --json
    if ($LASTEXITCODE -ne 0) {
        throw "installed fgx failed forgepack workflow evidence"
    }
} finally {
    Remove-Item -LiteralPath $installRoot -Recurse -Force
}
