$CargoArguments = @($args)


$ErrorActionPreference = "Stop"
$projectRoot = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$toolingRoot = Join-Path $projectRoot ".tooling"
$cargoHome = Join-Path $toolingRoot "cargo"
$rustupHome = Join-Path $toolingRoot "rustup"
$llvmRoot = Join-Path $toolingRoot "llvm-mingw-extracted\llvm-mingw-20260826-ucrt-x86_64"
$llvmBin = Join-Path $llvmRoot "bin"
$cargo = Join-Path $cargoHome "bin\cargo.exe"
$toolchain = "1.98.0-x86_64-pc-windows-gnu"
$target = "x86_64-pc-windows-gnu"

if (-not (Test-Path -LiteralPath $cargo) -or -not (Test-Path -LiteralPath $llvmBin)) {
    throw "Toolchain missing. Run .\scripts\windows\bootstrap-rust.ps1 first."
}
if ($CargoArguments.Count -eq 0) {
    throw "Pass a Cargo command, for example: test --workspace"
}

$env:RUSTUP_HOME = $rustupHome
$env:CARGO_HOME = $cargoHome
$env:LIBRARY_PATH = Join-Path $toolingRoot "gnu-link-shims"
$env:PATH = "$llvmBin;$env:PATH"
$env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = Join-Path $llvmBin "x86_64-w64-mingw32-clang.exe"

$targetedCommands = @("build", "check", "clippy", "run", "test")
$needsTarget = $targetedCommands -contains $CargoArguments[0] -and -not ($CargoArguments -contains "--target")

Push-Location $projectRoot
try {
    if (-not $needsTarget) {
        & $cargo "+$toolchain" @CargoArguments
        exit $LASTEXITCODE
    }

    $separator = [Array]::IndexOf($CargoArguments, "--")
    if ($separator -lt 0) {
        & $cargo "+$toolchain" @CargoArguments --target $target
    } else {
        $cargoSide = if ($separator -eq 0) { @() } else { $CargoArguments[0..($separator - 1)] }
        $programSide = if ($separator -eq ($CargoArguments.Count - 1)) { @() } else { $CargoArguments[($separator + 1)..($CargoArguments.Count - 1)] }
        & $cargo "+$toolchain" @cargoSide --target $target -- @programSide
    }
    exit $LASTEXITCODE
} finally {
    Pop-Location
}
