#!/usr/bin/env pwsh
# The single entry point. Everything runs in the container; nothing is built
# on the host machine.
param(
    [Parameter(Position = 0)][string]$Cmd = 'help',
    [Parameter(ValueFromRemainingArguments = $true)][string[]]$Rest
)

$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

function Invoke-InContainer([string[]]$CmdLine) {
    docker compose run --rm dev @CmdLine
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

switch ($Cmd) {
    'image'    { docker compose build dev }
    'build'    { Invoke-InContainer (@('cargo', 'build') + $Rest) }
    'test'     { Invoke-InContainer (@('cargo', 'test') + $Rest) }
    'check'    { Invoke-InContainer (@('cargo', 'clippy', '--all-targets', '--', '-D', 'warnings') + $Rest) }
    'fmt'      { Invoke-InContainer (@('cargo', 'fmt', '--all') + $Rest) }
    'fixtures' { Invoke-InContainer @('bash', 'tools/make-fixtures.sh') }
    'shell'    { docker compose run --rm dev bash }
    'win' {
        # Cross-compile a Windows .exe and copy it to ./dist, which is bind-mounted.
        Invoke-InContainer @('cargo', 'build', '--release', '--target', 'x86_64-pc-windows-gnu')
        Invoke-InContainer @('bash', '-lc', 'mkdir -p /work/dist && cp -v /work/target/x86_64-pc-windows-gnu/release/*.exe /work/dist/ 2>/dev/null || echo "(no binaries yet)"')
    }
    'linux' {
        Invoke-InContainer @('cargo', 'build', '--release')
        Invoke-InContainer @('bash', '-lc', 'mkdir -p /work/dist && find /work/target/release -maxdepth 1 -type f -executable -exec cp -v {} /work/dist/ \; 2>/dev/null || true')
    }
    default {
        Write-Host @"
Usage: .\x.ps1 <command>

  image      rebuild the docker image
  build      cargo build inside the container
  test       cargo test inside the container
  check      cargo clippy, warnings treated as errors
  fmt        cargo fmt
  fixtures   regenerate the gzip interop fixtures
  win        release build of the Windows .exe -> ./dist
  linux      release build for Linux -> ./dist
  shell      interactive bash inside the container
"@
    }
}
