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
    'bench'    { Invoke-InContainer (@('cargo', 'run', '-q', '--release', '-p', 'wp-cli', '--', 'bench') + $Rest) }
    'fixtures' { Invoke-InContainer @('bash', 'tools/make-fixtures.sh') }
    'corpus'   { Invoke-InContainer (@('cargo', 'run', '-q', '--release', '-p', 'wp-cli', '--', 'corpus') + $Rest) }
    'fidelity' { Invoke-InContainer (@('cargo', 'run', '-q', '--release', '-p', 'wp-cli', '--', 'fidelity') + $Rest) }
    'conformance' { Invoke-InContainer (@('cargo', 'run', '-q', '--release', '-p', 'wp-cli', '--', 'conformance') + $Rest) }
    'vba'      { Invoke-InContainer (@('cargo', 'run', '-q', '--release', '-p', 'wp-cli', '--', 'vba') + $Rest) }
    'word-check' {
        # The one command that runs here and not in the container: it drives
        # the Word installed on this machine, which the container cannot see.
        # It scores with dist\wp.exe, so .\x.ps1 win comes first.
        if (-not $Rest) { $Rest = @('corpus') }
        & (Join-Path $PSScriptRoot 'tools\word-check.ps1') @Rest
        exit $LASTEXITCODE
    }
    'shell'    { docker compose run --rm dev bash }
    'win' {
        # Cross-compile the Windows .exes and copy them to ./dist, which is
        # bind-mounted; then the installer, which carries them.
        Invoke-InContainer @('cargo', 'build', '--release', '--target', 'x86_64-pc-windows-gnu')
        Invoke-InContainer @('bash', '-lc', 'mkdir -p /work/dist && cp -v /work/target/x86_64-pc-windows-gnu/release/*.exe /work/dist/ 2>/dev/null || echo "(no binaries yet)"')
        Invoke-InContainer @('bash', 'tools/pack-installer.sh', 'win')
    }
    'linux' {
        Invoke-InContainer @('cargo', 'build', '--release')
        Invoke-InContainer @('bash', '-lc', 'mkdir -p /work/dist && find /work/target/release -maxdepth 1 -type f -executable -exec cp -v {} /work/dist/ \; 2>/dev/null || true')
        Invoke-InContainer @('bash', 'tools/pack-installer.sh', 'linux')
    }
    default {
        Write-Host @"
Usage: .\x.ps1 <command>

  image      rebuild the docker image
  build      cargo build inside the container
  test       cargo test inside the container
  check      cargo clippy, warnings treated as errors
  fmt        cargo fmt
  bench      time what a person waits for, on a document of N pages
  fixtures   regenerate the gzip interop fixtures
  corpus     open, save and compare every real document in .\corpus
  fidelity   score the pages drawn for them against Word's own
  conformance  run the Unicode test suites in .\unicode against the engine
  vba        read every macro in .\corpus and write it back out
  word-check open every document in .\corpus in Word and keep Word's pages
             for fidelity; runs on this machine, with its Word, not in Docker
  win        release build of the Windows .exe and its installer -> ./dist
  linux      release build for Linux and its installer -> ./dist
  shell      interactive bash inside the container
"@
    }
}
