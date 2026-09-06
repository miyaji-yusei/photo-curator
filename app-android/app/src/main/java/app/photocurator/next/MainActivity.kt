package app.photocurator.next

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.ContextCompat
import coil.compose.AsyncImage
import uniffi.photo_curator_core.BurstThreshold
import uniffi.photo_curator_core.PhotoRef
import uniffi.photo_curator_core.groupBursts
import coil.request.ImageRequest
import kotlinx.coroutines.launch

class MainActivity : ComponentActivity() {
    private val requestPhotos =
        registerForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { }

    override fun onCreate(savedInstanceState: Bundle?) {
        // 画面の端まで描き、余白はここではなく Compose の insets で取る。
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        requestPhotoPermissions()
        setContent { App() }
    }

    private fun requestPhotoPermissions() {
        val wanted = when {
            Build.VERSION.SDK_INT >= 34 -> arrayOf(
                Manifest.permission.READ_MEDIA_IMAGES,
                "android.permission.READ_MEDIA_VISUAL_USER_SELECTED"
            )
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU ->
                arrayOf(Manifest.permission.READ_MEDIA_IMAGES)
            else -> arrayOf(Manifest.permission.READ_EXTERNAL_STORAGE)
        }
        val missing = wanted.filter {
            ContextCompat.checkSelfPermission(this, it) != PackageManager.PERMISSION_GRANTED
        }
        if (missing.isNotEmpty()) requestPhotos.launch(missing.toTypedArray())
    }
}

/** 配色は Tauri 版と揃える。同じアプリだと分かること。 */
private val Ink = androidx.compose.ui.graphics.Color(0xFF101114)
private val Surface = androidx.compose.ui.graphics.Color(0xFF191B20)
private val Tile = androidx.compose.ui.graphics.Color(0xFF16181D)
private val Lime = androidx.compose.ui.graphics.Color(0xFFD6FF73)
private val Faint = androidx.compose.ui.graphics.Color(0xFF9AA0AA)

@Composable
private fun App() {
    MaterialTheme(colorScheme = darkColorScheme(primary = Lime, background = Ink, surface = Surface)) {
        var album by remember { mutableStateOf<Album?>(null) }
        Surface(color = Ink, modifier = Modifier.fillMaxSize()) {
            if (album == null) {
                AlbumList(onPick = { album = it })
            } else {
                Cull(album = album!!, onBack = { album = null })
            }
        }
    }
}

@Composable
private fun AlbumList(onPick: (Album) -> Unit) {
    val context = LocalContext.current
    var albums by remember { mutableStateOf<List<Album>>(emptyList()) }
    var note by remember { mutableStateOf("読み込み中…") }
    // **前面に戻るたびに読み直す。**
    // 権限のダイアログは別の Activity なので、許可した直後にここへ戻ってくる。
    // 起動時に 1 回だけ問い合わせると、許可前の「0 件」がそのまま残る。
    var reloads by remember { mutableStateOf(0) }
    val owner = LocalLifecycleOwner.current
    DisposableEffect(owner) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) reloads += 1
        }
        owner.lifecycle.addObserver(observer)
        onDispose { owner.lifecycle.removeObserver(observer) }
    }

    LaunchedEffect(reloads) {
        note = "読み込み中…"
        albums = Photos.albums(context)
        note = if (albums.isEmpty()) "写真が見つかりません（権限を確認してください）"
        else "アルバム ${albums.size} 件"
    }

    Column(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing)) {
        Row(
            Modifier.fillMaxWidth().height(56.dp).padding(horizontal = 16.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Text("Photo Curator", fontSize = 18.sp, fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.weight(1f))
            Text(note, fontSize = 12.sp, color = Faint)
        }
        LazyColumn(Modifier.fillMaxSize()) {
            items(albums, key = { it.id }) { album ->
                Row(
                    Modifier.fillMaxWidth().clickable { onPick(album) }.padding(16.dp, 10.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    AsyncImage(
                        model = ImageRequest.Builder(LocalContext.current)
                            .data(android.content.ContentUris.withAppendedId(
                                android.provider.MediaStore.Images.Media.EXTERNAL_CONTENT_URI,
                                album.coverId
                            ))
                            .size(160).build(),
                        contentDescription = null,
                        contentScale = ContentScale.Crop,
                        modifier = Modifier.size(48.dp).clip(RoundedCornerShape(8.dp)).background(Tile)
                    )
                    Spacer(Modifier.width(14.dp))
                    Column(Modifier.weight(1f)) {
                        Text(album.name, fontSize = 15.sp)
                        Text("${album.count} 枚", fontSize = 12.sp, color = Faint)
                    }
                }
            }
        }
    }
}

/**
 * 選別画面。**帯は上の 1 本だけ。残りは全部写真に使う。**
 * 行×列は枚数と表示領域の縦横比で決める（Tauri 版と同じ規則）。
 */
@Composable
private fun Cull(album: Album, onBack: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var photos by remember { mutableStateOf<List<Photo>>(emptyList()) }
    var index by remember { mutableStateOf(0) }
    var measured by remember { mutableStateOf("") }
    val groupSize = 4

    LaunchedEffect(album.id) {
        photos = Photos.photos(context, album.id)
    }

    // **まとめ方の判断は core（Rust）に任せる。**
    // PC・Web と同じコードなので、答えがずれない。
    val groups = remember(photos) {
        if (photos.isEmpty()) emptyList() else {
            val refs = photos.map {
                PhotoRef(
                    relativePath = it.relativePath,
                    capturedAt = it.takenAt,
                    // dHash はまだ作っていないので、いまは時間だけで切れる。
                    dHash = null,
                    dHashVersion = 2
                )
            }
            groupBursts(
                refs,
                BurstThreshold(windowMs = 4000, distance = 6u, dHashVersion = 2),
                emptyList()
            )
        }
    }

    val group = remember(photos, index) {
        photos.drop(index).take(groupSize)
    }

    Column(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing)) {
        Row(
            Modifier.fillMaxWidth().height(56.dp).padding(horizontal = 4.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            IconButton(onClick = onBack) {
                Icon(Icons.Filled.ArrowBack, contentDescription = "戻る")
            }
            Column(Modifier.weight(1f)) {
                Text(album.name, fontSize = 13.sp)
                Text(
                    "残り ${maxOf(0, photos.size - index)} 枚 · ${groups.size} グループ",
                    fontSize = 12.sp, color = Faint
                )
            }
            // **loadThumbnail の速さを測る。** 判断材料をその場で取る。
            TextButton(onClick = {
                scope.launch { measured = Photos.measureThumbnails(context, photos, 512) }
            }) { Text("計測", fontSize = 12.sp) }
            Button(
                onClick = { index = minOf(photos.size, index + groupSize) },
                shape = RoundedCornerShape(50),
                enabled = group.isNotEmpty()
            ) { Text("次へ", fontWeight = FontWeight.Bold) }
        }

        if (measured.isNotEmpty()) {
            Text(measured, fontSize = 11.sp, color = Lime, modifier = Modifier.padding(12.dp, 4.dp))
        }

        LazyVerticalGrid(
            columns = GridCells.Fixed(2),
            modifier = Modifier.weight(1f).padding(6.dp),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
            verticalArrangement = Arrangement.spacedBy(6.dp)
        ) {
            items(group, key = { it.id }) { photo ->
                Box(
                    Modifier
                        .aspectRatio(1f)
                        .clip(RoundedCornerShape(8.dp))
                        .background(Tile)
                ) {
                    AsyncImage(
                        model = ImageRequest.Builder(LocalContext.current)
                            .data(photo.uri)
                            // **要求した大きさでデコードする。**
                            // 原本を丸ごとメモリに載せない。
                            .size(1024)
                            .build(),
                        contentDescription = photo.name,
                        // 切らずに全部見せる。Tauri 版で黒帯になっていたところ。
                        contentScale = ContentScale.Fit,
                        modifier = Modifier.fillMaxSize()
                    )
                }
            }
        }
    }
}
