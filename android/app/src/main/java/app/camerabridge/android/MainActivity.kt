package app.camerabridge.android

import android.content.Intent
import android.os.Bundle
import android.provider.Settings
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.compose.foundation.Image
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.role
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.util.Locale

class MainActivity : ComponentActivity() {
    private val model by viewModels<CameraViewModel>()
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.auto(android.graphics.Color.TRANSPARENT, android.graphics.Color.TRANSPARENT),
            navigationBarStyle = SystemBarStyle.auto(android.graphics.Color.WHITE, android.graphics.Color.BLACK)
        )
        setContent { CameraBridgeApp(model) }
    }
    override fun onStop() {
        super.onStop()
        if (!isChangingConfigurations) model.stopForegroundWork()
    }
}

private val lightColors = lightColorScheme(
    primary = Color(0xFF007AFF), onPrimary = Color.White,
    secondary = Color(0xFF555555), onSecondary = Color.White,
    secondaryContainer = Color(0xFFF0F0EF), onSecondaryContainer = Color(0xFF202020),
    background = Color(0xFFF2F2F7), onBackground = Color(0xFF1C1C1E),
    surface = Color.White, onSurface = Color(0xFF202020),
    surfaceVariant = Color(0xFFF5F5F4), onSurfaceVariant = Color(0xFF737373),
    surfaceTint = Color.Transparent, outline = Color(0xFFD5D5D3), outlineVariant = Color(0xFFEAEAE8),
    error = Color(0xFFA03232), errorContainer = Color(0xFFFAEEEE), onErrorContainer = Color(0xFF862A2A)
)
private val darkColors = darkColorScheme(
    primary = Color(0xFF0A84FF), onPrimary = Color.White,
    background = Color.Black, onBackground = Color(0xFFF2F2F7),
    surface = Color(0xFF1C1C1E), onSurface = Color(0xFFF2F2F7),
    surfaceVariant = Color(0xFF2C2C2E), onSurfaceVariant = Color(0xFFAEAEB2),
    outlineVariant = Color(0xFF38383A), surfaceTint = Color.Transparent
)
private val colors: ColorScheme
    @Composable get() = MaterialTheme.colorScheme
private val buttonShape = RoundedCornerShape(12.dp)

@Composable
fun CameraBridgeApp(model: CameraViewModel) {
    var tab by rememberSaveable { mutableIntStateOf(0) }
    val state = model.state
    MaterialTheme(colorScheme = if (isSystemInDarkTheme()) darkColors else lightColors) {
        Scaffold(
            topBar = {
                Column(Modifier.fillMaxWidth().statusBarsPadding()) {
                    Row(Modifier.fillMaxWidth().height(60.dp).padding(horizontal = 24.dp), verticalAlignment = Alignment.CenterVertically) {
                        Text("CameraBridge", fontSize = 18.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.weight(1f))
                        Text(if (state.details?.demo == true) "演示" else "本地", fontSize = 12.sp, color = colors.onSurfaceVariant)
                    }
                    Rule()
                }
            },
            bottomBar = {
                NavigationBar(containerColor = colors.surface.copy(alpha = 0.96f), tonalElevation = 0.dp) {
                    val icons = listOf(R.drawable.ic_tab_camera, R.drawable.ic_tab_photos, R.drawable.ic_tab_transfer, R.drawable.ic_tab_settings)
                    listOf("相机", "照片", "传输", "设置").forEachIndexed { index, title ->
                        NavigationBarItem(selected = tab == index, onClick = { tab = index },
                            icon = { Icon(painterResource(icons[index]), null, Modifier.size(24.dp)) },
                            label = { Text(title, fontSize = 11.sp) },
                            colors = NavigationBarItemDefaults.colors(selectedIconColor = colors.primary,
                                selectedTextColor = colors.primary, indicatorColor = Color.Transparent,
                                unselectedIconColor = colors.onSurfaceVariant, unselectedTextColor = colors.onSurfaceVariant))
                    }
                }
            }
        ) { padding ->
            Column(Modifier.fillMaxSize().padding(padding)) {
                state.error?.let { error ->
                    Row(Modifier.fillMaxWidth().background(colors.errorContainer).padding(start = 24.dp, end = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                        Text(error, Modifier.weight(1f), color = colors.onErrorContainer, fontSize = 13.sp)
                        TextButton(onClick = model::dismissError) { Text("关闭", color = colors.onErrorContainer) }
                    }
                }
                when (tab) {
                    0 -> ConnectionScreen(model) { tab = 1 }
                    1 -> GalleryScreen(model) { tab = 2 }
                    2 -> TransfersScreen(model)
                    else -> AboutScreen()
                }
            }
        }
    }
}

@Composable
private fun ConnectionScreen(model: CameraViewModel, browse: () -> Unit) {
    val context = LocalContext.current
    val state = model.state
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(24.dp), verticalArrangement = Arrangement.spacedBy(24.dp)) {
        item { PageTitle("相机", "通过 Wi-Fi 导入照片") }
        item {
            Surface(shape = RoundedCornerShape(16.dp), color = colors.surface) {
            Row(Modifier.fillMaxWidth().padding(20.dp), verticalAlignment = Alignment.CenterVertically) {
                Icon(painterResource(R.drawable.ic_tab_camera), null, Modifier.size(36.dp), tint = colors.onSurface)
                Spacer(Modifier.width(16.dp))
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(state.details?.name ?: "Nikon Z6 II", fontSize = 20.sp, fontWeight = FontWeight.Medium)
                Text(state.details?.firmware ?: "C 1.70 · PTP/IP", color = colors.onSurfaceVariant, fontSize = 13.sp)
            }
            }
            }
        }
        if (state.details != null) {
            item {
                Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    Text(if (state.details.demo) "演示相机已连接 · 240 个示例对象" else "相机已连接 · ${state.total} 个对象", fontSize = 14.sp)
                    Button(onClick = browse, Modifier.fillMaxWidth().height(48.dp), shape = buttonShape) { Text("浏览照片") }
                    OutlinedButton(onClick = model::disconnect, enabled = !state.busy, modifier = Modifier.fillMaxWidth().height(48.dp), shape = buttonShape) { Text("断开连接") }
                }
            }
        } else {
            item {
                Row(Modifier.clip(RoundedCornerShape(16.dp)).background(colors.surface).padding(horizontal = 16.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        Text("演示模式", fontSize = 15.sp)
                        Text("无需连接设备", color = colors.onSurfaceVariant, fontSize = 12.sp)
                    }
                    Switch(checked = model.demo, onCheckedChange = { model.demo = it }, enabled = !state.busy)
                }
            }
            if (!model.demo) {
                item {
                    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                        Text("先加入相机 Wi-Fi，再填写相机地址。", color = colors.onSurfaceVariant, fontSize = 13.sp)
                        OutlinedButton(onClick = { context.startActivity(Intent(Settings.ACTION_WIFI_SETTINGS)) }, Modifier.fillMaxWidth().height(48.dp), enabled = !state.busy, shape = buttonShape) { Text("打开 Wi-Fi 设置") }
                        OutlinedTextField(value = model.host, onValueChange = { model.host = it }, label = { Text("相机 IP 地址") },
                            singleLine = true, enabled = !state.busy, modifier = Modifier.fillMaxWidth(), shape = buttonShape,
                            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri))
                        OutlinedTextField(value = model.port, onValueChange = { model.port = it }, label = { Text("PTP/IP 端口") },
                            singleLine = true, enabled = !state.busy, modifier = Modifier.fillMaxWidth(), shape = buttonShape,
                            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number))
                    }
                }
            }
            item {
                Button(onClick = model::connect, enabled = !state.busy, modifier = Modifier.fillMaxWidth().height(48.dp), shape = buttonShape) {
                    if (state.busy) { CircularProgressIndicator(Modifier.size(18.dp), color = colors.onPrimary, strokeWidth = 2.dp); Spacer(Modifier.width(10.dp)) }
                    Text(if (state.busy) "正在连接…" else "连接相机")
                }
            }
        }
        item { Text("传输时保持应用在前台。\nJPEG 保存到相册，NEF 保存到下载目录。", color = colors.onSurfaceVariant, fontSize = 12.sp, lineHeight = 21.sp) }
    }
}

@Composable
private fun GalleryScreen(model: CameraViewModel, transfers: () -> Unit) {
    val state = model.state
    if (state.details == null) { EmptyScreen("还没有连接相机", "在相机页面连接设备，或开启演示模式。"); return }
    Column(Modifier.fillMaxSize()) {
        Column(Modifier.padding(horizontal = 20.dp, vertical = 12.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("照片", fontSize = 28.sp, fontWeight = FontWeight.Bold, modifier = Modifier.weight(1f))
                TextButton(onClick = model::selectAll) { Text(if (state.selected.size == state.assets.size && state.assets.isNotEmpty()) "取消全选" else "全选") }
            }
            Text("已加载 ${state.assets.size} / ${state.total} 个对象" + if (state.details.demo) " · 演示" else "", color = colors.onSurfaceVariant, fontSize = 12.sp)
            Row(Modifier.fillMaxWidth().padding(top = 16.dp).clip(RoundedCornerShape(10.dp)).background(colors.surfaceVariant).padding(3.dp)) {
                MediaFilter.entries.forEach { filter ->
                    val selected = state.filter == filter
                    Box(Modifier.weight(1f).clip(RoundedCornerShape(8.dp)).background(if (selected) colors.surface else Color.Transparent)
                        .selectable(selected = selected, enabled = !state.loadingPage && !state.busy,
                        role = Role.Tab, onClick = { model.changeFilter(filter) }).heightIn(min = 44.dp), contentAlignment = Alignment.Center) {
                        Text(filter.title, fontSize = 14.sp, fontWeight = if (selected) FontWeight.SemiBold else FontWeight.Normal,
                            color = if (selected) colors.onSurface else colors.onSurfaceVariant)
                    }
                }
            }
        }
        LazyVerticalGrid(columns = GridCells.Adaptive(150.dp), modifier = Modifier.weight(1f),
            contentPadding = PaddingValues(horizontal = 20.dp, vertical = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(12.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            items(state.assets, key = { it.id }) { asset -> AssetTile(model, asset, asset.id in state.selected) }
            item(span = { GridItemSpan(maxLineSpan) }) {
                Column(Modifier.fillMaxWidth().padding(vertical = 16.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                    if (state.loadingPage) CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp)
                    else if (state.cursor != null) TextButton(onClick = { model.loadPage() }) { Text("加载更多") }
                    else if (state.assets.isEmpty()) {
                        Text("没有找到照片", fontSize = 14.sp)
                        TextButton(onClick = { model.loadPage(reset = true) }) { Text("重新加载") }
                    }
                }
            }
        }
        Rule()
        Row(Modifier.fillMaxWidth().background(colors.surface.copy(alpha = 0.96f)).padding(horizontal = 20.dp, vertical = 12.dp), verticalAlignment = Alignment.CenterVertically) {
            Text("已选 ${state.selected.size} 项", Modifier.weight(1f), fontSize = 13.sp, color = colors.onSurfaceVariant)
            Button(onClick = { model.downloadSelected(); if (model.state.transferring) transfers() },
                enabled = state.selected.isNotEmpty() && !state.transferring && !state.busy, shape = buttonShape) { Text("下载所选") }
        }
    }
}

@Composable
private fun AssetTile(model: CameraViewModel, asset: CameraAsset, selected: Boolean) {
    var loaded by remember(asset.id, model.state.details) { mutableStateOf(false) }
    val bitmap by produceState<android.graphics.Bitmap?>(null, asset.id, model.state.details) {
        value = model.thumbnail(asset)
        loaded = true
    }
    Column(Modifier.clickable { model.toggle(asset) }.semantics { this.selected = selected; role = Role.Checkbox },
        verticalArrangement = Arrangement.spacedBy(7.dp)) {
        Surface(shape = RoundedCornerShape(12.dp), border = if (selected) BorderStroke(2.dp, colors.primary) else null,
            color = colors.surfaceVariant, modifier = Modifier.fillMaxWidth().aspectRatio(4f / 3f)) {
            Box(contentAlignment = Alignment.Center) {
                if (bitmap != null) Image(bitmap!!.asImageBitmap(), "${asset.filename} 的缩略图",
                    Modifier.fillMaxSize().testTag("thumbnail-${asset.id}"), contentScale = ContentScale.Crop)
                else if (!loaded && asset.hasThumbnail) CircularProgressIndicator(Modifier.size(22.dp), strokeWidth = 2.dp)
                else Icon(painterResource(R.drawable.ic_tab_photos), "预览不可用", Modifier.size(28.dp), tint = colors.onSurfaceVariant)
                Surface(color = if (selected) colors.primary else Color.Black.copy(alpha = 0.25f), shape = androidx.compose.foundation.shape.CircleShape,
                    border = BorderStroke(1.5.dp, Color.White), modifier = Modifier.align(Alignment.TopEnd).padding(8.dp).size(24.dp)) {
                    if (selected) Icon(painterResource(R.drawable.ic_check), null, Modifier.padding(5.dp), tint = Color.White)
                }
            }
        }
        Text(asset.filename, fontSize = 12.sp, fontWeight = FontWeight.Medium, maxLines = 1)
        Text("${asset.kind.name} · ${bytesLabel(asset.size)}", fontSize = 11.sp, color = colors.onSurfaceVariant)
    }
}

@Composable
private fun TransfersScreen(model: CameraViewModel) {
    val state = model.state
    val context = LocalContext.current
    if (state.transfers.isEmpty()) { EmptyScreen("暂无传输记录", "在照片页面选择文件并下载。"); return }
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(24.dp)) {
        item {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("传输", Modifier.weight(1f), fontSize = 23.sp, fontWeight = FontWeight.Medium)
                if (state.transferring) TextButton(onClick = model::cancelTransfers) { Text("停止传输") }
            }
            Text("JPEG 保存到相册，NEF 保存到下载目录", color = colors.onSurfaceVariant, fontSize = 12.sp, modifier = Modifier.padding(top = 6.dp, bottom = 24.dp))
        }
        items(state.transfers, key = { it.id }) { item ->
            Rule()
            Column(Modifier.fillMaxWidth().padding(vertical = 18.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Row {
                    Text(item.asset.filename, Modifier.weight(1f), fontWeight = FontWeight.Medium, fontSize = 14.sp)
                    Text(item.status.title, color = if (item.status == TransferStatus.FAILED) colors.error else colors.onSurfaceVariant, fontSize = 12.sp)
                }
                Text(bytesLabel(item.asset.size), color = colors.onSurfaceVariant, fontSize = 12.sp)
                if (item.status in setOf(TransferStatus.DOWNLOADING, TransferStatus.SAVING, TransferStatus.QUEUED))
                    LinearProgressIndicator(progress = { item.progress }, modifier = Modifier.fillMaxWidth().height(2.dp))
                item.message?.let { Text(it, color = colors.onSurfaceVariant, fontSize = 12.sp) }
                if (item.stagingPath != null && item.status == TransferStatus.FAILED)
                    TextButton(onClick = { model.retrySave(item) }, enabled = !state.transferring && !state.busy) { Text("重试保存") }
                item.uri?.let { uri -> TextButton(onClick = {
                    val intent = Intent(Intent.ACTION_VIEW).setDataAndType(android.net.Uri.parse(uri), if (item.asset.kind == MediaKind.JPEG) "image/jpeg" else "image/x-nikon-nef")
                        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                    try { context.startActivity(intent) } catch (_: android.content.ActivityNotFoundException) {
                        android.widget.Toast.makeText(context, "已保存，可在相册或下载目录中查看", android.widget.Toast.LENGTH_SHORT).show()
                    }
                }) { Text("打开文件") } }
            }
        }
    }
}

@Composable
private fun AboutScreen() {
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(24.dp), verticalArrangement = Arrangement.spacedBy(24.dp)) {
        item { PageTitle("设置", "CameraBridge for Android · 0.1.0") }
        item { InfoSection("本地传输", "无需账号，没有云端服务或遥测。应用只连接指定的相机，不读取整个相册。") }
        item { InfoSection("设备支持", "Nikon Z6 II，C 固件 1.70。需要 Android 10 或更高版本的 64 位设备。实体相机兼容性仍待验收。") }
        item { InfoSection("保存位置", "JPEG：Pictures/CameraBridge\nNEF：Download/CameraBridge\n传输时保持应用在前台。保存失败后可在传输记录中重试。") }
        item { InfoSection("模拟测试", "演示模式仅测试分页和传输状态，不提供照片预览。照片缩略图仅从连接的相机读取。") }
    }
}
@Composable
private fun PageTitle(title: String, subtitle: String) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(title, fontSize = 28.sp, fontWeight = FontWeight.Bold)
        Text(subtitle, fontSize = 13.sp, color = colors.onSurfaceVariant)
    }
}
@Composable
private fun InfoSection(title: String, body: String) {
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Rule()
        Text(title, fontSize = 15.sp, fontWeight = FontWeight.Medium)
        Text(body, color = colors.onSurfaceVariant, fontSize = 13.sp, lineHeight = 22.sp)
    }
}
@Composable
private fun EmptyScreen(title: String, body: String) {
    Column(Modifier.fillMaxSize().padding(24.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Spacer(Modifier.height(60.dp))
        Text(title, fontSize = 20.sp, fontWeight = FontWeight.Medium)
        Text(body, color = colors.onSurfaceVariant, fontSize = 13.sp, lineHeight = 22.sp)
    }
}
@Composable
private fun Rule() { HorizontalDivider(color = colors.outlineVariant, thickness = 1.dp) }
private fun bytesLabel(bytes: Long): String = String.format(Locale.CHINA, "%.1f MB", bytes / 1_000_000.0)
