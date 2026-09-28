# Run in PowerShell on Windows with Rust MSVC and Visual Studio C++ Build Tools installed.
param([switch]$Run)
$ErrorActionPreference = "Stop"
Push-Location (Join-Path $PSScriptRoot "..")
try {
    & cargo fmt --all --check
    if ($LASTEXITCODE -ne 0) { throw "Rust formatting check failed" }
    & cargo test --locked --no-default-features
    if ($LASTEXITCODE -ne 0) { throw "Core/PTY tests failed" }
    & cargo check --locked
    if ($LASTEXITCODE -ne 0) { throw "Windows desktop check failed" }
    & cargo build --locked
    if ($LASTEXITCODE -ne 0) { throw "Windows debug build failed" }
    if ($Run) { & .\target\debug\mantash.exe }
} finally {
    Pop-Location
}
