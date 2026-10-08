param(
    [string]$NdkRoot = $env:ANDROID_NDK_HOME,
    [ValidateSet("arm64-v8a", "x86_64")]
    [string[]]$Abis = @("arm64-v8a", "x86_64")
)
$ErrorActionPreference = "Stop"
$projectRoot = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
if (-not $NdkRoot) { $NdkRoot = Join-Path $projectRoot ".tooling\android\android-ndk-r28b" }
$llvmBin = Join-Path $NdkRoot "toolchains\llvm\prebuilt\windows-x86_64\bin"
if (-not (Test-Path -LiteralPath $llvmBin)) { throw "缺少 Android NDK r28b。设置 ANDROID_NDK_HOME 或传入 -NdkRoot。" }
$env:RUSTUP_HOME = Join-Path $projectRoot ".tooling\rustup"
$env:CARGO_HOME = Join-Path $projectRoot ".tooling\cargo"
$env:PATH = "$(Join-Path $env:CARGO_HOME 'bin');$env:PATH"
$rustup = Join-Path $env:CARGO_HOME "bin\rustup.exe"
$targetNames = @{ "arm64-v8a" = "aarch64-linux-android"; "x86_64" = "x86_64-linux-android" }
$generated = Join-Path $projectRoot "android\app\src\main\java"
$jniRoot = Join-Path $projectRoot "android\app\src\main\jniLibs"
Push-Location $projectRoot
try {
    foreach ($abi in $Abis) {
        $target = $targetNames[$abi]
        & $rustup target add $target --toolchain 1.98.0-x86_64-pc-windows-gnu
        if ($LASTEXITCODE -ne 0) { throw "Rust target installation failed" }
        $envName = $target.ToUpper().Replace('-', '_')
        [Environment]::SetEnvironmentVariable("CARGO_TARGET_${envName}_LINKER", (Join-Path $llvmBin "${target}29-clang.cmd"), "Process")
        [Environment]::SetEnvironmentVariable("CARGO_TARGET_${envName}_RUSTFLAGS", "-C link-arg=-Wl,-z,max-page-size=16384", "Process")
        & "$projectRoot\scripts\windows\cargo.ps1" build --locked --release -p camera-ffi --target $target
        if ($LASTEXITCODE -ne 0) { throw "Rust Android build failed for $abi" }
        $destination = Join-Path $jniRoot $abi
        New-Item -ItemType Directory -Force $destination | Out-Null
        Copy-Item -LiteralPath "$projectRoot\target\$target\release\libcamera_ffi.so" -Destination $destination
    }
    $bindgen = Join-Path $projectRoot ".tooling\uniffi-cli-windows\bin\uniffi-bindgen.exe"
    if (-not (Test-Path -LiteralPath $bindgen)) {
        & "$projectRoot\scripts\windows\cargo.ps1" install uniffi --version 0.31.2 --features cli --locked --root "$projectRoot\.tooling\uniffi-cli-windows"
        if ($LASTEXITCODE -ne 0) { throw "UniFFI installation failed" }
    }
    # release 的 .so 会去掉符号；同轮构建的静态库保留 UniFFI 元数据。
    $bindingFile = "$generated\uniffi\camera_ffi\camera_ffi.kt"
    if (Test-Path -LiteralPath $bindingFile) { Remove-Item -LiteralPath $bindingFile }
    & $bindgen generate --library "$projectRoot\target\$($targetNames[$Abis[0]])\release\libcamera_ffi.a" --language kotlin --config "$projectRoot\android\uniffi.toml" --out-dir $generated --no-format
    if ($LASTEXITCODE -ne 0) { throw "Kotlin binding generation failed" }
    if (-not (Test-Path -LiteralPath $bindingFile)) { throw "No Kotlin binding was generated" }
} finally { Pop-Location }
