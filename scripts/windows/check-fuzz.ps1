[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$projectRoot = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$llvmCxx = Join-Path `
    $projectRoot `
    ".tooling\llvm-mingw-extracted\llvm-mingw-20260826-ucrt-x86_64\bin\x86_64-w64-mingw32-clang++.exe"

if (-not (Test-Path -LiteralPath $llvmCxx)) {
    throw "Toolchain missing. Run .\scripts\windows\bootstrap-rust.ps1 first."
}

# 即使使用 GNU Rust 目标，libFuzzer 的 Windows 适配层也会使用 __pragma。
# 启用 Microsoft 扩展后，Clang 可以接受该语法。
$env:CXX_x86_64_pc_windows_gnu = $llvmCxx
$env:CXXFLAGS_x86_64_pc_windows_gnu = "-fms-extensions"

& (Join-Path $PSScriptRoot "cargo.ps1") check --manifest-path fuzz\Cargo.toml --bins
