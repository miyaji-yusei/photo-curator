package app.photocurator.next

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Block
import androidx.compose.material.icons.filled.BrokenImage
import androidx.compose.material.icons.filled.Schedule
import androidx.compose.material.icons.filled.Sync
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * 絵が出ていないタイルの理由。**空白のまま置かない。**
 *
 * 「まだ作っている」のか「作れない」のかが分からないタイルは、待てばいいのか
 * 直せばいいのかも分からない。準備に時間がかかるアプリでは、この 1 語が
 * 「壊れている」と「動いている」を分ける。
 */
enum class Preview { Ready, Generating, Queued, Failed, Unsupported }

/** 端末も NAS も読めない形式。**拡張子で分かるものだけを言う。** */
private val UNREADABLE = setOf("arw", "cr2", "cr3", "nef", "orf", "raf", "rw2", "dng", "tif", "tiff")

fun unsupportedFormat(name: String): String? {
    val extension = name.substringAfterLast('.', "").lowercase()
    return if (extension in UNREADABLE) extension.uppercase() else null
}

/**
 * 写真で見る版。**Amazon の写真は名前で決めない。**
 *
 * 名前が `.cr2` でも中身は JPEG のことがある（実測で 454 枚すべて）。
 * 一覧を作るときに中身の種類（image/ で始まるもの）で選んであり、絵は Amazon が
 * JPEG に縮小して返すので、拡張子で「非対応」と言ってはいけない。
 */
fun unsupportedFormat(photo: Photo): String? =
    if (photo.amazon != null) null else unsupportedFormat(photo.name)

/**
 * 絵がまだ無いタイルの中身。
 *
 * `Ready` では何も描かない。読み込みが終われば写真がこの上に載る。
 */
@Composable
fun EmptyTile(state: Preview, format: String? = null) {
    if (state == Preview.Ready) return
    val (icon, label, tint) = when (state) {
        Preview.Generating -> Triple(Icons.Filled.Sync, "作成中", Sky)
        Preview.Queued -> Triple(Icons.Filled.Schedule, "待機", Faint)
        Preview.Failed -> Triple(Icons.Filled.BrokenImage, "読めません", Warn)
        else -> Triple(Icons.Filled.Block, "${format ?: "この形式"} 非対応", Faint)
    }
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center
        ) {
            Icon(icon, null, Modifier.size(18.dp), tint = tint)
            Text(
                label,
                fontSize = 10.sp, color = tint, textAlign = TextAlign.Center,
                modifier = Modifier.padding(top = 4.dp)
            )
        }
    }
}
