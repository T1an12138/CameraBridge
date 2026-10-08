package app.camerabridge.android

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.async
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runTest
import org.junit.Assert.*
import org.junit.Test

class DemoCameraClientTest {
    @Test fun pagingAndFormatFiltering() = runTest {
        val camera = DemoCameraClient()
        val seen = mutableSetOf<String>()
        var cursor: String? = null
        do {
            val page = camera.page(MediaFilter.ALL, cursor)
            assertEquals(240L, page.total)
            assertEquals(60, page.items.size)
            page.items.forEach { assertTrue(seen.add(it.id)) }
            cursor = page.cursor
        } while (cursor != null)
        assertEquals(240, seen.size)
        val jpeg = camera.page(MediaFilter.JPEG, null)
        assertEquals(120L, jpeg.total)
        assertTrue(jpeg.items.all { it.kind == MediaKind.JPEG })
        val raw = camera.page(MediaFilter.NEF, jpeg.cursor)
        assertEquals(60, raw.items.size)
        assertNull(raw.cursor)
        assertTrue(raw.items.all { it.kind == MediaKind.NEF })
    }
    @Test fun demoNeverExportsFakeMedia() = runTest {
        val camera = DemoCameraClient()
        val asset = camera.page(MediaFilter.JPEG, null).items.first()
        assertFalse(asset.hasThumbnail)
        assertNull(camera.thumbnail(asset))
        var last = 0L
        val file = camera.download(asset) { bytes, total -> assertTrue(bytes >= last); assertEquals(asset.size, total); last = bytes }
        assertNull(file)
        assertEquals(asset.size, last)
    }
    @OptIn(kotlinx.coroutines.ExperimentalCoroutinesApi::class)
    @Test fun cancelThenReuseSession() = runTest {
        val camera = DemoCameraClient()
        val asset = camera.page(MediaFilter.NEF, null).items.first()
        val download = async { camera.download(asset) { _, _ -> } }
        advanceTimeBy(200)
        download.cancel()
        try { download.await(); fail("Cancellation expected") } catch (_: CancellationException) { }
        assertEquals(60, camera.page(MediaFilter.ALL, null).items.size)
        camera.disconnect()
        try { camera.page(MediaFilter.ALL, null); fail("Disconnected session must reject requests") } catch (_: IllegalStateException) { }
    }
}
