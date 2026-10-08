package app.camerabridge.android

import android.app.Application
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.util.LruCache
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.*
import kotlinx.coroutines.sync.Semaphore
import kotlinx.coroutines.sync.withPermit
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.util.UUID

enum class TransferStatus(val title: String) {
    QUEUED("等待中"), DOWNLOADING("正在下载"), SAVING("正在保存"), SAVED("已保存"),
    DEMO("演示完成"), CANCELLED("已取消"), FAILED("失败")
}
data class TransferItem(
    val id: String = UUID.randomUUID().toString(), val asset: CameraAsset,
    val status: TransferStatus = TransferStatus.QUEUED, val progress: Float = 0f,
    val message: String? = null, val uri: String? = null, val stagingPath: String? = null
)
data class CameraUiState(
    val busy: Boolean = false, val details: CameraDetails? = null,
    val assets: List<CameraAsset> = emptyList(), val selected: Set<String> = emptySet(),
    val filter: MediaFilter = MediaFilter.ALL, val cursor: String? = null, val total: Long = 0,
    val loadingPage: Boolean = false, val error: String? = null, val transfers: List<TransferItem> = emptyList(),
    val transferring: Boolean = false
)

class CameraViewModel(application: Application) : AndroidViewModel(application) {
    private val prefs = application.getSharedPreferences("camera", 0)
    private val staging = File(application.filesDir, "staging").apply { mkdirs() }
    private val exporter = MediaExporter(application)
    private val connectivity = application.getSystemService(ConnectivityManager::class.java)
    private val clientId = prefs.getString("clientId", null) ?: UUID.randomUUID().toString().also {
        prefs.edit().putString("clientId", it).apply()
    }
    var host by mutableStateOf(prefs.getString("host", "192.168.1.1") ?: "192.168.1.1")
    var port by mutableStateOf("15740")
    var demo by mutableStateOf(false)
    var state by mutableStateOf(CameraUiState(transfers = readHistory()))
        private set
    private var client: CameraClient? = null
    private var controlJob: Job? = null
    private var pageJob: Job? = null
    private var queueJob: Job? = null
    private var epoch = 0
    private val thumbnails = LruCache<String, Bitmap>(40)
    private val thumbnailSlots = Semaphore(2)

    fun connect() {
        if (state.busy || state.details != null) return
        val useDemo = demo
        val address = host.trim()
        val endpointPort = port.toIntOrNull()
        if (!useDemo && (address.isEmpty() || endpointPort == null || endpointPort !in 1..65535)) {
            state = state.copy(error = "请填写相机 IP 和有效端口（1–65535）")
            return
        }
        state = state.copy(busy = true, error = null)
        val generation = ++epoch
        controlJob = viewModelScope.launch {
            var candidate: CameraClient? = null
            try {
                if (!useDemo) {
                    // 相机热点没有互联网；把本进程的原生 Rust socket 路由到 Wi-Fi。
                    val wifi = connectivity.allNetworks.firstOrNull {
                        connectivity.getNetworkCapabilities(it)?.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) == true
                    }
                    if (wifi != null) check(connectivity.bindProcessToNetwork(wifi)) { "无法使用 Wi-Fi 网络" }
                    else check(address in setOf("127.0.0.1", "localhost", "10.0.2.2")) { "请先在系统设置中加入相机 Wi-Fi" }
                }
                candidate = if (useDemo) DemoCameraClient() else RustCameraClient.connect(address, endpointPort!!, clientId, staging)
                val details = candidate.info()
                val firstPage = candidate.page(state.filter, null)
                ensureActive()
                if (generation != epoch) return@launch
                client = candidate
                candidate = null
                prefs.edit().putString("host", address).apply()
                state = state.copy(details = details, assets = firstPage.items, cursor = firstPage.cursor,
                    total = firstPage.total, selected = emptySet())
            } catch (cancel: CancellationException) { throw cancel }
            catch (error: Exception) { state = state.copy(error = error.cameraMessage()) }
            finally {
                withContext(NonCancellable) { runCatching { candidate?.disconnect() } }
                if (client == null) connectivity.bindProcessToNetwork(null)
                state = state.copy(busy = false)
            }
        }
    }

    fun disconnect() {
        if (state.busy) return
        state = state.copy(busy = true)
        controlJob = viewModelScope.launch {
            ++epoch
            pageJob?.cancelAndJoin()
            queueJob?.cancelAndJoin()
            val previous = client
            client = null
            try { previous?.disconnect() }
            catch (cancel: CancellationException) { throw cancel }
            catch (error: Exception) { state = state.copy(error = error.cameraMessage()) }
            finally {
                connectivity.bindProcessToNetwork(null)
                thumbnails.evictAll()
                state = state.copy(busy = false, details = null, assets = emptyList(), selected = emptySet(),
                    cursor = null, total = 0, loadingPage = false)
            }
        }
    }

    fun changeFilter(filter: MediaFilter) {
        if (state.loadingPage || state.busy || filter == state.filter) return
        state = state.copy(filter = filter, selected = emptySet(), assets = emptyList(), cursor = null, total = 0)
        loadPage(reset = true)
    }
    fun loadPage(reset: Boolean = false) {
        val current = client ?: return
        if (state.loadingPage || (!reset && state.cursor == null)) return
        val cursor = if (reset) null else state.cursor
        val generation = epoch
        val filter = state.filter
        state = state.copy(loadingPage = true, error = null)
        pageJob = viewModelScope.launch {
            try {
                val page = current.page(filter, cursor)
                if (generation == epoch) state = state.copy(
                    assets = ((if (reset) emptyList() else state.assets) + page.items).distinctBy { it.id },
                    cursor = page.cursor, total = page.total)
            } catch (cancel: CancellationException) { throw cancel }
            catch (error: Exception) { if (generation == epoch) state = state.copy(error = error.cameraMessage()) }
            finally { if (generation == epoch) state = state.copy(loadingPage = false) }
        }
    }
    fun toggle(asset: CameraAsset) {
        state = state.copy(selected = if (asset.id in state.selected) state.selected - asset.id else state.selected + asset.id)
    }
    fun selectAll() {
        state = state.copy(selected = if (state.selected.size == state.assets.size) emptySet() else state.assets.map { it.id }.toSet())
    }
    suspend fun thumbnail(asset: CameraAsset): Bitmap? {
        val current = client ?: return null
        val generation = epoch
        val key = "$generation:${asset.id}"
        thumbnails.get(key)?.let { return it }
        if (!asset.hasThumbnail) return null
        return thumbnailSlots.withPermit {
            try {
                if (generation != epoch) return@withPermit null
                val bytes = current.thumbnail(asset) ?: return@withPermit null
                val bitmap = withContext(Dispatchers.Default) {
                    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
                    BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
                    val options = BitmapFactory.Options().apply {
                        inSampleSize = maxOf(1, maxOf(bounds.outWidth, bounds.outHeight) / 400)
                    }
                    BitmapFactory.decodeByteArray(bytes, 0, bytes.size, options)
                }
                if (generation == epoch && bitmap != null) thumbnails.put(key, bitmap)
                if (generation == epoch) bitmap else null
            } catch (cancel: CancellationException) { throw cancel }
            catch (_: Exception) { null }
        }
    }

    fun downloadSelected() {
        val current = client ?: return
        if (state.transferring || state.busy) return
        val pending = state.assets.filter { it.id in state.selected }.map { TransferItem(asset = it) }
        if (pending.isEmpty()) return
        if (pending.size > 100) {
            state = state.copy(error = "每批最多下载 100 项，请减少所选文件")
            return
        }
        val history = pending + state.transfers
        state = state.copy(transfers = (history.take(100) + history.drop(100).filter { it.stagingPath != null }),
            selected = emptySet(), transferring = true, error = null)
        persistHistory()
        queueJob = viewModelScope.launch {
            try {
                for (item in pending) {
                    var staged: File? = null
                    try {
                        update(item.id) { it.copy(status = TransferStatus.DOWNLOADING) }
                        staged = current.download(item.asset) { bytes, total ->
                            update(item.id) { it.copy(progress = if (total > 0) (bytes.toDouble() / total).toFloat().coerceIn(0f, 1f) else 0f) }
                        }
                        if (staged == null) update(item.id) { it.copy(status = TransferStatus.DEMO, progress = 1f, message = "演示传输，未保存文件") }
                        else {
                            val completedFile = staged
                            update(item.id) { it.copy(status = TransferStatus.SAVING, stagingPath = completedFile.absolutePath) }
                            persistHistory()
                            // 已开始写相册时完成原子发布，再响应用户取消。
                            withContext(NonCancellable) {
                                val uri = exporter.save(item.asset, completedFile)
                                update(item.id) { it.copy(status = TransferStatus.SAVED, progress = 1f, uri = uri.toString(), stagingPath = null) }
                                completedFile.delete()
                                persistHistory()
                            }
                        }
                    } catch (cancel: CancellationException) {
                        update(item.id) { if (it.status == TransferStatus.SAVED) it else it.copy(status = TransferStatus.CANCELLED, message = "传输已停止") }
                        throw cancel
                    } catch (error: Exception) {
                        update(item.id) { it.copy(status = TransferStatus.FAILED, message = error.cameraMessage(), stagingPath = staged?.absolutePath) }
                    }
                    persistHistory()
                }
            } finally {
                state = state.copy(transferring = false, transfers = state.transfers.map {
                    if (it.status == TransferStatus.QUEUED) it.copy(status = TransferStatus.CANCELLED, message = "队列已停止") else it
                })
                persistHistory()
            }
        }
    }
    fun cancelTransfers() { queueJob?.cancel() }
    fun stopForegroundWork() {
        cancelTransfers()
        if (state.busy && state.details == null) controlJob?.cancel()
    }
    fun retrySave(item: TransferItem) {
        if (state.transferring || state.busy) return
        val file = item.stagingPath?.let(::File) ?: return
        if (file.parentFile?.canonicalFile != staging.canonicalFile) return
        state = state.copy(transferring = true)
        queueJob = viewModelScope.launch {
            try {
                update(item.id) { it.copy(status = TransferStatus.SAVING, message = null) }
                withContext(NonCancellable) {
                    val uri = exporter.save(item.asset, file)
                    update(item.id) { it.copy(status = TransferStatus.SAVED, uri = uri.toString(), stagingPath = null, progress = 1f) }
                    file.delete()
                }
            } catch (error: Exception) { update(item.id) { it.copy(status = TransferStatus.FAILED, message = error.cameraMessage()) } }
            finally { state = state.copy(transferring = false); persistHistory() }
        }
    }
    fun dismissError() { state = state.copy(error = null) }
    private fun update(id: String, transform: (TransferItem) -> TransferItem) {
        state = state.copy(transfers = state.transfers.map { if (it.id == id) transform(it) else it })
    }
    private fun persistHistory() {
        val history = JSONArray()
        state.transfers.forEach { item -> history.put(JSONObject().apply {
            put("id", item.id); put("assetId", item.asset.id); put("filename", item.asset.filename)
            put("kind", item.asset.kind.name); put("size", item.asset.size); put("status", item.status.name)
            put("message", item.message); put("uri", item.uri); put("staging", item.stagingPath)
        }) }
        prefs.edit().putString("history", history.toString()).apply()
    }
    private fun readHistory(): List<TransferItem> = try {
        val history = JSONArray(prefs.getString("history", "[]"))
        (0 until history.length()).map { index ->
            val record = history.getJSONObject(index)
            val status = TransferStatus.valueOf(record.getString("status"))
            val interrupted = status in setOf(TransferStatus.QUEUED, TransferStatus.DOWNLOADING, TransferStatus.SAVING)
            TransferItem(record.getString("id"), CameraAsset(record.getString("assetId"), record.getString("filename"),
                MediaKind.valueOf(record.getString("kind")), record.getLong("size")),
                if (interrupted) TransferStatus.FAILED else status,
                if (status in setOf(TransferStatus.SAVED, TransferStatus.DEMO)) 1f else 0f,
                if (interrupted) "上次运行已中断，可重新连接下载或重试保存" else record.optString("message").takeIf { it.isNotEmpty() },
                record.optString("uri").takeIf { it.isNotEmpty() }, record.optString("staging").takeIf { it.isNotEmpty() })
        }
    } catch (_: Exception) { emptyList() }
    override fun onCleared() {
        val previous = client
        client = null
        connectivity.bindProcessToNetwork(null)
        if (previous != null) CoroutineScope(Dispatchers.IO).launch { runCatching { previous.disconnect() } }
    }
}
