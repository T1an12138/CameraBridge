# Nikon Z6 II 硬件验证

## 固定测试目标

- 相机：Nikon Z6 II
- C 固件：1.70
- 传输方式：相机创建的 Wi-Fi 接入点
- 媒体：仅 JPEG 和 NEF

## 安全与隐私

请使用测试存储卡或测试图片。不要提交拍摄内容、Wi-Fi 凭据、诊断信息、EXIF、缩略图或已下载媒体。CLI 会对发起方标识符进行脱敏，且不会向诊断信息写入任何载荷字节。

## 运行前准备

1. 在**设定菜单 → 固件版本**中确认固件版本。
2. 在存储卡中放入至少 500 个测试对象，用于分页测试。
3. 启用相机的直连 Wi-Fi，并从 Windows 手动加入该网络。
4. 如果 VPN 软件会接管相机所在子网，请暂时将其停用。
5. 使用 `Get-NetIPConfiguration` 确认选中的 Wi-Fi 接口和网关；保留的证据中不要包含 SSID 或凭据。
6. 显式设置相机地址；CameraBridge 不会扫描子网。
7. 执行任何下载前，先运行端点检查和探测：

```powershell
$cameraAddress = "<actual-camera-ipv4>"

.\scripts\windows\cargo.ps1 run -p camera-cli '--' network `
  --host $cameraAddress --port 15740 --connect-timeout-ms 15000

.\scripts\windows\cargo.ps1 run -p camera-cli '--' probe `
  --host $cameraAddress --port 15740 `
  --connect-timeout-ms 15000 --request-timeout-ms 15000
```

## 验收运行

准备存储卡，确保枚举出的前 50 个对象同时包含 JPEG 和 NEF。验证器目前会下载前 50 个对象；它不会强制按格式设置配额。

请使用仓库外的目标目录，避免意外提交测试媒体：

```powershell
$cameraAddress = "<actual-camera-ipv4>"
$runRoot = "D:\CameraBridge-validation\z6ii-1.70"
New-Item -ItemType Directory -Force -Path $runRoot | Out-Null

.\scripts\windows\cargo.ps1 run -p camera-cli '--' validate `
  --host $cameraAddress `
  --port 15740 `
  --connect-timeout-ms 15000 `
  --request-timeout-ms 15000 `
  --cycles 20 `
  --minimum-items 500 `
  --max-p95-ready-ms 15000 `
  --download-count 50 `
  --required-disconnect-recoveries 3 `
  --destination (Join-Path $runRoot "downloads") `
  --report (Join-Path $runRoot "validation-report.json")
```

在三个不同文件的传输过程中中断相机 Wi-Fi 链路，并在相应文件的单次重连尝试结束前恢复连接。当前文件会在 500 ms 后从第零字节重新开始，并重试一次；已完成的文件不会重复传输。除非以下检查全部通过，否则命令会以非零状态退出：

1. 全部 20 轮连接/列目录/断开循环均完成，至少 19 轮首次尝试成功，且任何失败的首次尝试都能在一次重试内恢复。
2. 从开始连接到获得首个媒体页面的 p95 不超过 15 秒。
3. 枚举结果稳定，且至少包含 500 个唯一对象。
4. 全部 50 个下载均完成，至少三个中断的下载成功恢复，并且重复下载的第一个对象具有相同的 SHA-256。

完成后：

1. 确认下载集合确实同时包含 JPEG 和 NEF。
2. 确认不存在误标为完成的项目或重复的账本标识。
3. 通过 USB 或读卡器单独复制重复下载的样本并比较 SHA-256；完成此步骤后才能宣布字节准确性已验证。

## 应保留的证据

仅保留脱敏后的 JSON 摘要：模式/应用/OS 版本、相机型号/固件、时间阈值、连接与下载恢复次数、对象数量、字节数和哈希。绝不要在仓库中保留媒体或凭据值。
