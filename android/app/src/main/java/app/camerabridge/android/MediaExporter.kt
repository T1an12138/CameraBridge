package app.camerabridge.android

import android.content.ContentValues
import android.content.Context
import android.net.Uri
import android.os.Environment
import android.provider.MediaStore
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.File

class MediaExporter(private val context: Context) {
    suspend fun save(asset: CameraAsset, file: File): Uri = withContext(Dispatchers.IO) {
        check(file.isFile && file.length() == asset.size) { "文件大小与相机记录不一致" }
        val jpeg = asset.kind == MediaKind.JPEG
        val collection = if (jpeg) MediaStore.Images.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)
            else MediaStore.Downloads.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)
        val values = ContentValues().apply {
            put(MediaStore.MediaColumns.DISPLAY_NAME, asset.filename)
            put(MediaStore.MediaColumns.MIME_TYPE, if (jpeg) "image/jpeg" else "image/x-nikon-nef")
            put(MediaStore.MediaColumns.RELATIVE_PATH,
                (if (jpeg) Environment.DIRECTORY_PICTURES else Environment.DIRECTORY_DOWNLOADS) + "/CameraBridge")
            put(MediaStore.MediaColumns.IS_PENDING, 1)
        }
        val resolver = context.contentResolver
        val uri = checkNotNull(resolver.insert(collection, values)) { "无法创建媒体文件" }
        try {
            checkNotNull(resolver.openOutputStream(uri)).use { output -> file.inputStream().use { it.copyTo(output) } }
            check(resolver.update(uri, ContentValues().apply { put(MediaStore.MediaColumns.IS_PENDING, 0) }, null, null) == 1)
            uri
        } catch (error: Exception) {
            resolver.delete(uri, null, null)
            throw error
        }
    }
}
