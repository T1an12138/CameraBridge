# 解析器模糊测试

这些测试目标会在 1 MiB 分配上限下同时测试完整帧解析和分片流解析。
由于 `cargo-fuzz` 使用 `nightly` 编译器和检测器插桩，
这些测试目标与 `stable` 工作区隔离。

使用项目的 Windows 工具链对两个测试目标执行编译检查：

```powershell
.\scripts\windows\check-fuzz.ps1
```

```bash
cargo install cargo-fuzz --locked
cargo +nightly fuzz run ptp_container -- -max_len=1048576
cargo +nightly fuzz run ptpip_packet -- -max_len=1048576
```

不要将实体相机的抓包数据加入语料库。分享任何复现样本前，
请先将其最小化并脱敏。
