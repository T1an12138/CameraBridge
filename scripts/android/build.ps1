param([string]$NdkRoot = $env:ANDROID_NDK_HOME, [string]$SdkRoot = $env:ANDROID_HOME)
$ErrorActionPreference = "Stop"
$projectRoot = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
if (-not $SdkRoot) { $SdkRoot = $env:ANDROID_SDK_ROOT }
if (-not $SdkRoot) { $SdkRoot = Join-Path $projectRoot ".tooling\android\sdk" }
if (-not (Test-Path -LiteralPath "$SdkRoot\platforms\android-36\android.jar")) {
    throw "缺少 Android SDK platform 36。设置 ANDROID_HOME 或传入 -SdkRoot。"
}
$env:GRADLE_USER_HOME = Join-Path $projectRoot ".tooling\android\gradle-home"
$env:ANDROID_USER_HOME = Join-Path $projectRoot ".tooling\android\user-home"
New-Item -ItemType Directory -Force $env:ANDROID_USER_HOME | Out-Null
Set-Content -LiteralPath "$projectRoot\android\local.properties" -Value ("sdk.dir=" + $SdkRoot.Replace('\', '/').Replace(':', '\:')) -Encoding utf8
& "$PSScriptRoot\build-native.ps1" -NdkRoot $NdkRoot
if ($LASTEXITCODE -ne 0) { throw "Native build failed" }
Push-Location "$projectRoot\android"
try {
    $localGradle = Join-Path $projectRoot ".tooling\android\gradle-8.13\bin\gradle.bat"
    $gradleCommand = if (Test-Path -LiteralPath $localGradle) { $localGradle } else { "$projectRoot\android\gradlew.bat" }
    & $gradleCommand :app:assembleDebug :app:testDebugUnitTest :app:lintDebug --console=plain --no-daemon
    if ($LASTEXITCODE -ne 0) { throw "Android build or checks failed" }
} finally { Pop-Location }
$output = Join-Path $projectRoot "target\android-artifacts"
New-Item -ItemType Directory -Force $output | Out-Null
$apk = Join-Path $output "CameraBridge-Android-0.1.0-debug.apk"
Copy-Item -LiteralPath "$projectRoot\android\app\build\outputs\apk\debug\app-debug.apk" -Destination $apk -Force
$hash = (Get-FileHash -LiteralPath $apk -Algorithm SHA256).Hash.ToLower()
Set-Content -LiteralPath "$output\SHA256SUMS" -Value "$hash  CameraBridge-Android-0.1.0-debug.apk" -Encoding ascii
Write-Output "APK: $apk"
Write-Output "SHA256: $hash"
