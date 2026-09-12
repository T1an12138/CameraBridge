[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$projectRoot = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$toolingRoot = Join-Path $projectRoot ".tooling"
$rustupHome = Join-Path $toolingRoot "rustup"
$cargoHome = Join-Path $toolingRoot "cargo"
$rustupInit = Join-Path $toolingRoot "rustup-init.exe"
$llvmArchive = Join-Path $toolingRoot "llvm-mingw.zip"
$llvmExtract = Join-Path $toolingRoot "llvm-mingw-extracted"
$llvmFolder = Join-Path $llvmExtract "llvm-mingw-20260826-ucrt-x86_64"
$rustToolchain = "1.98.0-x86_64-pc-windows-gnu"

New-Item -ItemType Directory -Force -Path $toolingRoot | Out-Null

if (-not (Test-Path -LiteralPath $rustupInit)) {
    Invoke-WebRequest `
        -Uri "https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-gnu/rustup-init.exe" `
        -OutFile $rustupInit
}

$env:RUSTUP_HOME = $rustupHome
$env:CARGO_HOME = $cargoHome

if (-not (Test-Path -LiteralPath (Join-Path $cargoHome "bin\cargo.exe"))) {
    & $rustupInit -y --no-modify-path --profile minimal --default-toolchain $rustToolchain
}

$rustup = Join-Path $cargoHome "bin\rustup.exe"
& $rustup component add clippy rustfmt --toolchain $rustToolchain

if (-not (Test-Path -LiteralPath $llvmFolder)) {
    if (-not (Test-Path -LiteralPath $llvmArchive)) {
        Invoke-WebRequest `
            -Uri "https://github.com/mstorsjo/llvm-mingw/releases/download/20260826/llvm-mingw-20260826-ucrt-x86_64.zip" `
            -OutFile $llvmArchive
    }
    New-Item -ItemType Directory -Force -Path $llvmExtract | Out-Null
    Expand-Archive -LiteralPath $llvmArchive -DestinationPath $llvmExtract -Force
}

$shimRoot = Join-Path $toolingRoot "gnu-link-shims"
New-Item -ItemType Directory -Force -Path $shimRoot | Out-Null
Copy-Item `
    -LiteralPath (Join-Path $llvmFolder "x86_64-w64-mingw32\lib\libunwind.a") `
    -Destination (Join-Path $shimRoot "libgcc_eh.a") `
    -Force
Copy-Item `
    -LiteralPath (Join-Path $llvmFolder "lib\clang\23\lib\windows\libclang_rt.builtins-x86_64.a") `
    -Destination (Join-Path $shimRoot "libgcc.a") `
    -Force

Write-Host "CameraBridge Rust/LLVM toolchain is ready."
Write-Host "Run: .\scripts\windows\cargo.ps1 test --workspace"
