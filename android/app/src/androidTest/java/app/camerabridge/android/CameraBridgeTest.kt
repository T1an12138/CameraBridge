package app.camerabridge.android

import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import uniffi.camera_ffi.*
import java.io.File
import java.util.UUID

class CameraBridgeTest {
    @get:Rule val compose = createAndroidComposeRule<MainActivity>()

    @Test fun demoConnectBrowseAndTransfer() {
        compose.runOnIdle { androidx.lifecycle.ViewModelProvider(compose.activity)[CameraViewModel::class.java].demo = true }
        compose.onNodeWithText("连接相机").performScrollTo().performClick()
        compose.waitUntil(10_000) { compose.onAllNodesWithText("浏览照片").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithText("浏览照片").performScrollTo().performClick()
        compose.onNodeWithText("已加载 60 / 240 个对象 · 演示").assertExists()
        compose.onAllNodesWithContentDescription("预览不可用", useUnmergedTree = true).onFirst().assertIsDisplayed()
        compose.onNodeWithText("NEF").performClick()
        compose.waitUntil(10_000) { compose.onAllNodesWithText("已加载 60 / 120 个对象 · 演示").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithText("JPEG").performClick()
        compose.waitUntil(10_000) { compose.onAllNodesWithText("DSC_0001.JPG").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithText("DSC_0001.JPG").performClick()
        compose.onNodeWithText("下载所选").performClick()
        compose.waitUntil(10_000) { compose.onAllNodesWithText("演示完成").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithText("演示传输，未保存文件").assertExists()
    }


    @Test fun invalidCameraThumbnailShowsUnavailable() {
        val host = InstrumentationRegistry.getArguments().getString("mockHost")
        org.junit.Assume.assumeNotNull(host)
        lateinit var model: CameraViewModel
        compose.runOnIdle {
            model = androidx.lifecycle.ViewModelProvider(compose.activity)[CameraViewModel::class.java]
            model.demo = false
            model.host = requireNotNull(host)
            model.connect()
        }
        compose.waitUntil(20_000) { compose.onAllNodesWithText("浏览照片").fetchSemanticsNodes().isNotEmpty() }
        val firstAssetID = model.state.assets.first().id
        compose.onNodeWithText("浏览照片").performScrollTo().performClick()
        // 协议测试对端仅返回 JPEG 起止标记，不能解码；界面必须显示不可用而不是替代照片。
        compose.waitUntil(20_000) {
            compose.onAllNodesWithContentDescription("预览不可用", useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty()
        }
        compose.onNodeWithTag("thumbnail-$firstAssetID", useUnmergedTree = true).assertDoesNotExist()
        compose.onAllNodesWithContentDescription("预览不可用", useUnmergedTree = true).onFirst().assertIsDisplayed()
        compose.runOnIdle { model.disconnect() }
        compose.waitUntil(10_000) { model.state.details == null }
    }

    @Test fun nativeCoreLoadsAndReturnsStructuredErrors(): Unit = runBlocking {
        repeat(100) { CameraCore().destroy() }
        val core = CameraCore()
        try {
            try {
                core.connect(FfiConnectionConfig("127.0.0.1", 15740u, "Android test", UUID.randomUUID().toString(), 500uL, 500uL, "relative/path"))
                fail("Relative staging path must fail")
            } catch (error: CameraFfiException.Failure) {
                assertEquals(FfiErrorCode.INVALID_INPUT, error.code)
                assertTrue(error.reason.contains("staging_root"))
            }
        } finally { core.destroy() }
    }

    @Test fun exportsOwnMediaWithoutLibraryPermissions(): Unit = runBlocking {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val exporter = MediaExporter(context)
        for (kind in MediaKind.entries) {
            val bytes = if (kind == MediaKind.JPEG) {
                java.io.ByteArrayOutputStream().use { stream ->
                    val bitmap = android.graphics.Bitmap.createBitmap(4, 4, android.graphics.Bitmap.Config.ARGB_8888)
                    bitmap.eraseColor(android.graphics.Color.GREEN)
                    bitmap.compress(android.graphics.Bitmap.CompressFormat.JPEG, 90, stream)
                    bitmap.recycle()
                    stream.toByteArray()
                }
            } else byteArrayOf(0x49, 0x49, 0x2A, 0, 8, 0, 0, 0, 0, 0)
            val file = File.createTempFile("export-test", ".bin", context.cacheDir).apply { writeBytes(bytes) }
            val asset = CameraAsset("test", "CameraBridge-test-${UUID.randomUUID()}.${if (kind == MediaKind.JPEG) "JPG" else "NEF"}", kind, bytes.size.toLong())
            var uri: android.net.Uri? = null
            try {
                uri = exporter.save(asset, file)
                assertArrayEquals(bytes, context.contentResolver.openInputStream(uri)!!.use { it.readBytes() })
                context.contentResolver.query(uri, arrayOf(android.provider.MediaStore.MediaColumns.RELATIVE_PATH), null, null, null)!!.use { cursor ->
                    assertTrue(cursor.moveToFirst())
                    assertEquals(if (kind == MediaKind.JPEG) "Pictures/CameraBridge/" else "Download/CameraBridge/", cursor.getString(0))
                }
            } finally { uri?.let { context.contentResolver.delete(it, null, null) }; file.delete() }
        }
    }

    @Test fun nativeMockRoundtrip(): Unit = runBlocking {
        val host = InstrumentationRegistry.getArguments().getString("mockHost")
        org.junit.Assume.assumeNotNull(host)
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val staging = File(context.cacheDir, "ffi-roundtrip").apply { mkdirs() }
        val camera = RustCameraClient.connect(requireNotNull(host), 15740, UUID.randomUUID().toString(), staging)
        try {
            assertTrue(camera.info().name.contains("Nikon"))
            val first = camera.page(MediaFilter.ALL, null)
            assertEquals(60, first.items.size)
            val second = camera.page(MediaFilter.ALL, first.cursor)
            assertTrue(first.items.map { it.id }.toSet().intersect(second.items.map { it.id }.toSet()).isEmpty())
            val asset = first.items.first()
            val file = requireNotNull(camera.download(asset) { _, _ -> })
            assertEquals(asset.size, file.length())
            assertNotNull(camera.thumbnail(asset))
            file.delete()
        } finally { camera.disconnect() }
    }
}
