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
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
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
import coil.request.ImageRequest

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
val Ink = androidx.compose.ui.graphics.Color(0xFF101114)
val Surface = androidx.compose.ui.graphics.Color(0xFF191B20)
val Tile = androidx.compose.ui.graphics.Color(0xFF16181D)
val Lime = androidx.compose.ui.graphics.Color(0xFFD6FF73)
val Faint = androidx.compose.ui.graphics.Color(0xFF9AA0AA)

@Composable
private fun App() {
    MaterialTheme(colorScheme = darkColorScheme(primary = Lime, background = Ink, surface = Surface)) {
        var album by remember { mutableStateOf<Album?>(null) }
        Surface(color = Ink, modifier = Modifier.fillMaxSize()) {
            if (album == null) {
                AlbumList(onPick = { album = it })
            } else {
                CullScreen(album = album!!, onBack = { album = null })
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
