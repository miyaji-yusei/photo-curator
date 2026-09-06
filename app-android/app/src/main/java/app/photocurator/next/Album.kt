package app.photocurator.next

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material.icons.filled.ChevronRight
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.launch
import uniffi.photo_curator_core.Session

/**
 * アルバムの様子。**選別の前と後にここへ戻ってくる。**
 *
 * 作り直す前は、選別を終えても行き先が無かった。星は付くのに、
 * それがどこに溜まっているのか画面から分からず、「本当に効いているのか」
 * を確かめる方法が無かった。この画面がその答えを持つ。
 */
@Composable
fun AlbumScreen(
    album: Album,
    onBack: () -> Unit,
    onCull: () -> Unit,
    onRestart: () -> Unit,
    onOpenStar: (Int) -> Unit
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var session by remember { mutableStateOf<Session?>(null) }
    var loaded by remember { mutableStateOf(false) }
    var confirmRestart by remember { mutableStateOf(false) }
    // 戻ってくるたびに読み直す。**選別してきた結果が古いまま残らないように。**
    var reloads by remember { mutableStateOf(0) }
    // 一度に並べる枚数。**覚えておく。** 毎回選ばせるほどのことではない。
    var groupSize by remember { mutableStateOf(Prefs.groupSize(context)) }

    LaunchedEffect(album.id, reloads) {
        session = Store.load(context, album.id)
        loaded = true
    }

    Column(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing)) {
        Reading {
        Row(
            Modifier.fillMaxWidth().height(56.dp).padding(end = 16.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            IconButton(onClick = onBack) { Icon(Icons.Filled.ArrowBack, "アルバム一覧へ") }
            Column(Modifier.weight(1f)) {
                Text(album.name, fontSize = 16.sp, fontWeight = FontWeight.SemiBold)
                Text("${album.count} 枚", fontSize = 12.sp, color = Faint)
            }
        }
        }

        val live = session
        if (!loaded) {
            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                Text("読み込み中…", color = Faint, fontSize = 13.sp)
            }
        } else Reading(Modifier.padding(horizontal = 16.dp)) {
            if (live == null) {
                Card(
                    "まだ選別していません",
                    "似た写真をまとめてから、$groupSize 枚ずつ見比べます。" +
                        "途中でやめても続きから戻れます。"
                )
                Spacer(Modifier.height(16.dp))

                // **始める前にだけ選ばせる。** 途中で変えると、同じラウンドの
                // 中で見比べる枚数が変わってしまい、比べた条件が揃わなくなる。
                Text("一度に並べる枚数", fontSize = 12.sp, color = Faint)
                Spacer(Modifier.height(6.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    for (size in listOf(2, 3, 4, 6, 9)) {
                        FilterChip(
                            selected = size == groupSize,
                            onClick = {
                                groupSize = size
                                Prefs.setGroupSize(context, size)
                            },
                            label = { Text("$size 枚", fontSize = 13.sp) },
                            // 選んだものはこのアプリの色で示す。既定の紫は
                            // 他の場所で使っていないので、別の意味に見える。
                            colors = FilterChipDefaults.filterChipColors(
                                selectedContainerColor = Lime,
                                selectedLabelColor = Color.Black
                            )
                        )
                    }
                }

                Spacer(Modifier.height(16.dp))
                Button(
                    onClick = onCull, shape = RoundedCornerShape(50),
                    modifier = Modifier.fillMaxWidth().height(48.dp)
                ) {
                    Text("選別をはじめる", fontWeight = FontWeight.Bold)
                }
            } else {

            // ---- 途中か、終わったか ----
            val remaining = live.queue.size + live.current.size
            if (live.finished) {
                Card(
                    "ROUND ${live.round} まで終わりました",
                    "続けると ★${live.targetStar + 1} を選びます。"
                )
            } else {
                Card(
                    "ROUND ${live.round} の途中",
                    "★${live.targetStar} を選別中。残り $remaining 組。"
                )
            }
            Spacer(Modifier.height(12.dp))
            Button(
                onClick = onCull, shape = RoundedCornerShape(50),
                modifier = Modifier.fillMaxWidth().height(48.dp)
            ) {
                Text(if (live.finished) "選別を続ける" else "続きから", fontWeight = FontWeight.Bold)
            }

            Spacer(Modifier.height(20.dp))

            // ---- 星の溜まり具合 ----
            Text("星ごとの枚数", fontSize = 12.sp, color = Faint)
            Spacer(Modifier.height(6.dp))
            val counts = IntArray(6)
            for (star in live.ratings.values) counts[star.coerceIn(0, 5)] += 1
            // 上から。**多く星が付いたものが上**にある方が探しやすい。
            for (star in 5 downTo 0) {
                val count = counts[star]
                StarRow(
                    star = star,
                    count = count,
                    total = live.ratings.size,
                    // 0 枚のところへは行かせない。開いても何も無い。
                    onClick = if (count > 0) ({ onOpenStar(star) }) else null
                )
            }

            Spacer(Modifier.height(20.dp))
            TextButton(onClick = { confirmRestart = true }) {
                Text("最初からやり直す", fontSize = 13.sp, color = Faint)
            }
            }
        }
    }

    if (confirmRestart) {
        AlertDialog(
            onDismissRequest = { confirmRestart = false },
            title = { Text("選別を最初からやり直しますか") },
            // **何が消えるかを具体的に言う。** 「よろしいですか」では判断できない。
            text = {
                Text(
                    "これまでに付けた星と、どこまで見たかが消えます。" +
                        "写真そのものには手を触れません。"
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    confirmRestart = false
                    scope.launch {
                        Store.clear(context, album.id)
                        reloads += 1
                        onRestart()
                    }
                }) { Text("やり直す") }
            },
            dismissButton = {
                TextButton(onClick = { confirmRestart = false }) { Text("やめる") }
            }
        )
    }
}

@Composable
private fun Card(title: String, body: String) {
    Column(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(14.dp))
            .background(Surface)
            .padding(16.dp)
    ) {
        Text(title, fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
        Text(body, fontSize = 13.sp, color = Faint, modifier = Modifier.padding(top = 4.dp))
    }
}

@Composable
private fun StarRow(star: Int, count: Int, total: Int, onClick: (() -> Unit)?) {
    Row(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(10.dp))
            .then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier)
            .padding(horizontal = 10.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Text(
            if (star == 0) "★0" else "★$star",
            fontSize = 14.sp,
            color = if (count > 0) Lime else Faint,
            modifier = Modifier.width(44.dp)
        )
        // 割合を細い帯で。数字だけより、どこに溜まっているかが一目で分かる。
        Box(
            Modifier
                .weight(1f)
                .height(6.dp)
                .clip(RoundedCornerShape(3.dp))
                .background(Color(0xFF24272D))
        ) {
            if (count > 0 && total > 0) {
                Box(
                    Modifier
                        .fillMaxWidth(count.toFloat() / total)
                        .fillMaxHeight()
                        .clip(RoundedCornerShape(3.dp))
                        .background(if (star == 0) Faint else Lime)
                )
            }
        }
        Text(
            "$count 枚",
            fontSize = 13.sp,
            color = if (count > 0) Color.White else Faint,
            modifier = Modifier.width(72.dp).padding(start = 12.dp)
        )
        if (onClick != null) {
            Icon(Icons.Filled.ChevronRight, null, Modifier.size(18.dp), tint = Faint)
        } else {
            Spacer(Modifier.size(18.dp))
        }
    }
}
