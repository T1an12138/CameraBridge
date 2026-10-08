package app.camerabridge.android

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.withContext
import java.io.File
import java.util.UUID
import uniffi.camera_ffi.*

enum class MediaKind { JPEG, NEF }
enum class MediaFilter(val title: String) { ALL("全部"), JPEG("JPEG"), NEF("NEF") }
data class CameraAsset(val id: String, val filename: String, val kind: MediaKind, val size: Long, val hasThumbnail: Boolean = true)
data class MediaPage(val items: List<CameraAsset>, val cursor: String?, val total: Long)
data class CameraDetails(val name: String, val firmware: String, val demo: Boolean = false)

interface CameraClient {
    suspend fun info(): CameraDetails
    suspend fun page(filter: MediaFilter, cursor: String?): MediaPage
    suspend fun thumbnail(asset: CameraAsset): ByteArray?
    suspend fun download(asset: CameraAsset, progress: (Long, Long) -> Unit): File?
    suspend fun disconnect()
}

class RustCameraClient private constructor(private val session: FfiCameraSession, private val staging: File) : CameraClient {
    companion object {
        suspend fun connect(host: String, port: Int, clientId: String, staging: File): RustCameraClient {
            require(host.isNotBlank()) { "请输入相机 IP 地址" }
            require(port in 1..65535) { "端口应为 1–65535" }
            val core = CameraCore()
            try {
                return RustCameraClient(core.connect(FfiConnectionConfig(
                    host.trim(), port.toUShort(), "CameraBridge Android", clientId,
                    8_000uL, 15_000uL, staging.absolutePath
                )), staging)
            } finally { core.destroy() }
        }
    }
    override suspend fun info(): CameraDetails = session.info().let {
        CameraDetails(it.displayName, it.firmwareVersion ?: "未知固件")
    }
    override suspend fun page(filter: MediaFilter, cursor: String?): MediaPage {
        val formats = when (filter) {
            MediaFilter.ALL -> listOf(FfiMediaFormat.JPEG, FfiMediaFormat.NEF)
            MediaFilter.JPEG -> listOf(FfiMediaFormat.JPEG)
            MediaFilter.NEF -> listOf(FfiMediaFormat.NEF)
        }
        return session.listMedia(formats, 60u, cursor).let { page ->
            MediaPage(page.items.map {
                CameraAsset(it.id, it.filename, if (it.format == FfiMediaFormat.JPEG) MediaKind.JPEG else MediaKind.NEF,
                    it.sizeBytes.toLong(), it.thumbnailAvailable)
            }, page.nextCursor, page.totalItems?.toLong() ?: page.items.size.toLong())
        }
    }
    override suspend fun thumbnail(asset: CameraAsset): ByteArray? =
        if (asset.hasThumbnail) session.loadThumbnail(asset.id, 2_000_000uL).data else null

    override suspend fun download(asset: CameraAsset, progress: (Long, Long) -> Unit): File {
        // 私有暂存文件使用随机 UUID，不使用相机文件名或相册路径。
        // Android SELinux 禁止应用创建硬链接；overwrite=true 选择核心的原子 rename。
        val filename = "${UUID.randomUUID()}-${asset.kind.name.lowercase()}"
        val transfer = session.startDownload(asset.id, filename, asset.size.toULong(), true)
        try {
            while (true) {
                val snapshot = transfer.snapshot()
                progress(snapshot.bytesTransferred.toLong(), snapshot.totalBytes.toLong())
                when (snapshot.phase) {
                    FfiTransferPhase.COMPLETED -> return File(requireNotNull(snapshot.destinationPath))
                    FfiTransferPhase.CANCELLED -> throw CancellationException("传输已取消")
                    FfiTransferPhase.FAILED -> error(snapshot.errorMessage ?: "传输失败")
                    else -> delay(150)
                }
            }
        } finally {
            withContext(NonCancellable + Dispatchers.IO) {
                try {
                    transfer.cancel()
                    if (transfer.wait().phase != FfiTransferPhase.COMPLETED) File(staging, "$filename.partial").delete()
                } finally { transfer.destroy() }
            }
        }
    }
    override suspend fun disconnect() {
        try { session.disconnect() } finally { session.destroy() }
    }
}

fun Exception.cameraMessage(): String = if (this is CameraFfiException.Failure) when (code) {
    FfiErrorCode.CONNECTION_FAILED -> "无法连接相机，请检查热点和 IP 地址"
    FfiErrorCode.TIMEOUT -> "相机响应超时，请保持相机唤醒并重试"
    FfiErrorCode.CAMERA_BUSY -> "相机正在忙，请稍后重试"
    FfiErrorCode.CAMERA_ASLEEP -> "请唤醒相机后重新连接"
    FfiErrorCode.UNSUPPORTED_FIRMWARE -> "当前固件尚未适配：$reason"
    FfiErrorCode.INSUFFICIENT_SPACE -> "存储空间不足，请释放空间后重试"
    else -> reason
} else message ?: "操作失败，请重试"

// 演示对象不会生成或导出伪造的 JPEG/NEF 文件。
class DemoCameraClient : CameraClient {
    private var closed = false
    private val assets = (1..120).flatMap { number ->
        MediaKind.entries.map { kind ->
            CameraAsset("demo-$number-$kind", "DSC_%04d.%s".format(number, if (kind == MediaKind.JPEG) "JPG" else "NEF"),
                kind, if (kind == MediaKind.JPEG) 2_000_000 else 8_000_000, false)
        }
    }
    private fun checkConnected() = check(!closed) { "相机会话已断开" }
    override suspend fun info(): CameraDetails { checkConnected(); return CameraDetails("Nikon Z6 II", "C 1.70 · 演示", true) }
    override suspend fun page(filter: MediaFilter, cursor: String?): MediaPage {
        checkConnected()
        delay(180)
        val filtered = assets.filter { filter == MediaFilter.ALL || it.kind.name == filter.name }
        val start = cursor?.toIntOrNull() ?: 0
        require(start in 0..filtered.size) { "无效分页位置" }
        val end = minOf(start + 60, filtered.size)
        return MediaPage(filtered.subList(start, end), if (end < filtered.size) end.toString() else null, filtered.size.toLong())
    }
    override suspend fun thumbnail(asset: CameraAsset): ByteArray? {
        checkConnected()
        require(assets.any { it.id == asset.id }) { "未知演示照片" }
        return null
    }
    override suspend fun download(asset: CameraAsset, progress: (Long, Long) -> Unit): File? {
        repeat(20) { checkConnected(); delay(70); progress(asset.size * (it + 1) / 20, asset.size) }
        return null
    }
    override suspend fun disconnect() { closed = true }
}
