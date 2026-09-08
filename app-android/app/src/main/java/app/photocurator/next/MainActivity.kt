package app.photocurator.next

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.SystemBarStyle
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
        // **明暗は端末の設定に合わせない。** 既定の enableEdgeToEdge() は
        // 端末が明るいテーマだとナビゲーションバーに白い膜を敷く。この画面は
        // 常に暗いので、下端だけ白く残って「見切れている」ように見えていた。
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.dark(android.graphics.Color.TRANSPARENT),
            navigationBarStyle = SystemBarStyle.dark(android.graphics.Color.TRANSPARENT)
        )
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

/**
 * いまどの画面か。**ホーム → プロジェクト → 選別 / 結果** の 1 本道。
 *
 * 単位はアルバムではなくプロジェクト。同じアルバムから 2 つ作れるし、
 * 出所が端末でも NAS でも同じ道を通る。
 */
private sealed interface Screen {
    data object Home : Screen
    data object Settings : Screen
    data class Detail(val project: Project) : Screen
    data class Cull(val project: Project) : Screen
    data class Results(val project: Project, val star: Int) : Screen
}

@Composable
private fun App() {
    MaterialTheme(colorScheme = darkColorScheme(primary = Lime, background = Ink, surface = Surface)) {
        var screen by remember { mutableStateOf<Screen>(Screen.Home) }
        var creating by remember { mutableStateOf(false) }
        // ホームへ戻るたびに一覧を読み直すための鍵。
        var homeKey by remember { mutableStateOf(0) }

        Surface(color = Ink, modifier = Modifier.fillMaxSize()) {
            when (val here = screen) {
                is Screen.Home -> key(homeKey) {
                    HomeScreen(
                        onOpen = { screen = Screen.Detail(it) },
                        onCreate = { creating = true },
                        onSettings = { screen = Screen.Settings }
                    )
                }

                is Screen.Settings -> SettingsScreen(
                    onBack = { screen = Screen.Home; homeKey += 1 }
                )

                is Screen.Detail -> ProjectScreen(
                    project = here.project,
                    onBack = { screen = Screen.Home; homeKey += 1 },
                    onCull = { screen = Screen.Cull(here.project) },
                    onOpenStar = { screen = Screen.Results(here.project, it) }
                )

                is Screen.Cull -> CullScreen(
                    project = here.project,
                    // **選別から戻る先はプロジェクト詳細。** 一覧まで飛ばすと、
                    // いま何枚残ったのかを確かめる前に見失う。
                    onBack = { screen = Screen.Detail(here.project) }
                )

                is Screen.Results -> ResultsScreen(
                    project = here.project,
                    star = here.star,
                    onBack = { screen = Screen.Detail(here.project) }
                )
            }
        }

        if (creating) {
            CreateSheet(
                onCreated = { project ->
                    creating = false
                    // 作ったらそのまま詳細へ。準備の様子が見える。
                    screen = Screen.Detail(project)
                },
                onDismiss = { creating = false }
            )
        }
    }
}
