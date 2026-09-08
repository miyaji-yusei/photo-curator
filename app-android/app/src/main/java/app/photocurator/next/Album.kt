@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package app.photocurator.next

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Dns
import androidx.compose.material.icons.filled.Smartphone
import androidx.compose.material.icons.filled.Sync
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import coil.compose.AsyncImage
import coil.request.ImageRequest
import kotlinx.coroutines.launch
import uniffi.photo_curator_core.Session

/**
 * プロジェクト詳細。**状態を確かめて選別へ。主ボタンは 1 つ。**
 *
 * 左に「出所・準備・選別」の 3 カード、右に写真の一覧。
 * 一覧の役割は**走査と準備の結果を確かめること**で、選別のあとは星の確認。
 */
@Composable
fun ProjectScreen(
    project: Project,
    onBack: () -> Unit,
    onCull: (learn: Boolean) -> Unit,
    onOpenStar: (Int) -> Unit
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()

    var photos by remember { mutableStateOf<List<Photo>>(emptyList()) }
    var session by remember { mutableStateOf<Session?>(null) }
    var scanned by remember { mutableStateOf(false) }
    var prints by remember { mutableStateOf<Map<String, Fingerprint>>(emptyMap()) }
    var filter by remember { mutableStateOf("all") }
    var menu by remember { mutableStateOf(false) }
    var confirmRestart by remember { mutableStateOf(false) }
    var reloads by remember { mutableStateOf(0) }
    // 開始前の確認を出しているか。**初回は必ず出す。**
    var starting by remember { mutableStateOf(false) }

    // **開いたら準備が動き出す。** カードに数が出ているのに何も進まないと、
    // 止まっているのか終わっているのか分からない。
    var preparing by remember { mutableStateOf(0 to 0) }

    LaunchedEffect(project.id, reloads) {
        scanned = false
        session = Store.load(context, project.id)
        prints = Fingerprints.load(context, project.source.key)
        photos = Photos.forSource(context, project.source)
        scanned = true
        // 走査が終わってから指紋。**できた分から選別に出せる。**
        val ready = Prepare.run(context, project) { done, total -> preparing = done to total }
        photos = ready.first
        prints = Fingerprints.load(context, project.source.key)
    }

    val live = session
    val ratings = live?.ratings ?: emptyMap()
    val starred = photos.count { (ratings[it.relativePath] ?: 0) > 0 }
    val bursts = live?.members?.size ?: 0

    Column(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing)) {
        // ---- バー: ← / 名前 / … / 主ボタン（状態で 1 つ） ----
        Row(
            Modifier.fillMaxWidth().height(64.dp).padding(end = 16.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            IconButton(onClick = onBack) { Icon(Icons.Filled.ArrowBack, "ホームへ") }
            Text(project.name, fontSize = 18.sp, fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.weight(1f))
            IconButton(onClick = { menu = true }) { Icon(Icons.Filled.MoreVert, "その他") }
            Spacer(Modifier.width(8.dp))
            Button(
                onClick = {
                    // 初回か、「毎回確認」が on のときだけ挟む。
                    if (live == null || Prefs.askBeforeStart(context)) starting = true
                    else onCull(false)
                },
                // **走査が終わるまで始められない。** 枚数と時間順が決まらないため。
                enabled = scanned && photos.isNotEmpty(),
                shape = RoundedCornerShape(50)
            ) {
                Icon(Icons.Filled.PlayArrow, null, Modifier.size(18.dp))
                Spacer(Modifier.width(6.dp))
                Text(
                    when {
                        !scanned -> "走査中…"
                        live == null -> "選別を開始"
                        live.finished -> "結果を見る"
                        else -> "選別を続ける"
                    },
                    fontWeight = FontWeight.Bold, fontSize = 14.sp
                )
            }
        }

        Row(Modifier.fillMaxSize().padding(horizontal = 16.dp)) {
            // ---- 左: 出所 / 準備 / 選別 ----
            Column(Modifier.width(340.dp).verticalScrollable()) {
                Card {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Icon(
                            if (project.source.kind == "nas") Icons.Filled.Dns
                            else Icons.Filled.Smartphone,
                            null, Modifier.size(16.dp), tint = Faint
                        )
                        Spacer(Modifier.width(8.dp))
                        Text(project.source.label, fontSize = 13.sp)
                    }
                    Text(
                        "${photos.size} 枚",
                        fontSize = 12.sp, color = Faint,
                        modifier = Modifier.padding(top = 6.dp)
                    )
                    HorizontalDivider(Modifier.padding(vertical = 10.dp), color = Color(0xFF24272D))
                    Row(verticalAlignment = Alignment.Top) {
                        Icon(Icons.Filled.Lock, null, Modifier.size(14.dp), tint = Faint)
                        Spacer(Modifier.width(6.dp))
                        Text(
                            "原本は読むだけ。移動・削除・書き換えはしません",
                            fontSize = 11.sp, color = Faint
                        )
                    }
                }

                Spacer(Modifier.height(12.dp))

                // 準備。**常に done / total で出す。** 終わらないバーは出さない。
                Card {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text("準備", fontSize = 14.sp, fontWeight = FontWeight.SemiBold)
                        Spacer(Modifier.weight(1f))
                        Text(
                            if (scanned && prints.size >= photos.size) "完了" else "進行中",
                            fontSize = 12.sp, color = Lime
                        )
                    }
                    Spacer(Modifier.height(10.dp))
                    PrepRow("写真の走査", if (scanned) photos.size else 0, photos.size, scanned)
                    PrepRow("撮影時刻・サムネイル", if (scanned) photos.size else 0, photos.size, scanned)
                    // Tauri 版の「表示用画像」に当たる段。ネイティブでは OS の縮小画像を
                    // そのまま使うので、実際に作るのは連写のまとめに使う指紋だけ。
                    PrepRow(
                        "連写の指紋",
                        maxOf(prints.size, preparing.first),
                        photos.size,
                        prints.size >= photos.size && photos.isNotEmpty()
                    )
                    Text(
                        "できた写真から選別に出ます",
                        fontSize = 11.sp, color = Faint,
                        modifier = Modifier.padding(top = 8.dp)
                    )
                }

                Spacer(Modifier.height(12.dp))

                Card {
                    Text("選別", fontSize = 14.sp, fontWeight = FontWeight.SemiBold)
                    Spacer(Modifier.height(8.dp))
                    if (live == null) {
                        Text("まだ始めていません", fontSize = 13.sp, color = Faint)
                    } else {
                        val remaining = (live.queue + live.current)
                            .sumOf { live.members[it]?.size ?: 1 }
                        Text(
                            if (live.finished) "選別完了 · ROUND ${live.round}"
                            else "★${live.targetStar} を選別中 · ROUND ${live.round}",
                            fontSize = 13.sp, color = Lime, fontWeight = FontWeight.SemiBold
                        )
                        Text(
                            "残り $remaining 枚 / ${live.queue.size + live.current.size} グループ",
                            fontSize = 12.sp, color = Faint,
                            modifier = Modifier.padding(top = 4.dp)
                        )
                        Text(
                            "★1 以上 $starred 枚 · ${live.groupSize} 枚ずつ · 連写まとめ on",
                            fontSize = 12.sp, color = Faint,
                            modifier = Modifier.padding(top = 6.dp)
                        )
                    }
                    Spacer(Modifier.height(10.dp))
                    // 星ごとの内訳。押すと結果画面へ。
                    val counts = IntArray(6)
                    for (star in ratings.values) counts[star.coerceIn(0, 5)] += 1
                    for (star in 5 downTo 0) {
                        if (counts[star] == 0) continue
                        Row(
                            Modifier
                                .fillMaxWidth()
                                .clip(RoundedCornerShape(8.dp))
                                .clickable { onOpenStar(star) }
                                .padding(vertical = 6.dp, horizontal = 4.dp),
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            Text("★$star", fontSize = 13.sp, color = if (star > 0) Lime else Faint)
                            Spacer(Modifier.weight(1f))
                            Text("${counts[star]} 枚", fontSize = 13.sp)
                        }
                    }
                }
            }

            Spacer(Modifier.width(16.dp))

            // ---- 右: 写真の一覧。走査と準備の結果を確かめる場所 ----
            Column(Modifier.weight(1f)) {
                Row(
                    Modifier.padding(bottom = 8.dp),
                    horizontalArrangement = Arrangement.spacedBy(8.dp)
                ) {
                    Chip("すべて ${photos.size}", filter == "all") { filter = "all" }
                    if (starred > 0) Chip("★1 以上 $starred", filter == "star") { filter = "star" }
                    if (bursts > 0) Chip("連写 $bursts 組", filter == "burst") { filter = "burst" }
                }

                val shown = when (filter) {
                    "star" -> photos.filter { (ratings[it.relativePath] ?: 0) > 0 }
                    "burst" -> {
                        val inBurst = live?.members?.values?.flatten()?.toSet() ?: emptySet()
                        photos.filter { it.relativePath in inBurst }
                    }
                    else -> photos
                }
                LazyVerticalGrid(
                    columns = GridCells.Adaptive(minSize = 96.dp),
                    verticalArrangement = Arrangement.spacedBy(4.dp),
                    horizontalArrangement = Arrangement.spacedBy(4.dp)
                ) {
                    items(shown, key = { it.id }) { photo ->
                        Box(
                            Modifier
                                .aspectRatio(1f)
                                .clip(RoundedCornerShape(6.dp))
                                .background(Tile)
                        ) {
                            AsyncImage(
                                model = ImageRequest.Builder(LocalContext.current)
                                    .data(photo.thumbModel).size(256).build(),
                                contentDescription = photo.name,
                                imageLoader = Images.loader(LocalContext.current),
                                contentScale = ContentScale.Crop,
                                modifier = Modifier.fillMaxSize()
                            )
                            val star = ratings[photo.relativePath] ?: 0
                            if (star > 0) {
                                Text(
                                    "★$star",
                                    fontSize = 11.sp, color = Lime,
                                    modifier = Modifier
                                        .align(Alignment.BottomStart)
                                        .padding(4.dp)
                                        .clip(RoundedCornerShape(4.dp))
                                        .background(Color(0xB3101114))
                                        .padding(horizontal = 5.dp, vertical = 1.dp)
                                )
                            }
                        }
                    }
                }
            }
        }
    }

    if (starting) {
        StartSheet(
            project = project,
            photoCount = photos.size,
            onStart = {
                starting = false
                // 連写をまとめる設定で、まだ基準を決めていなければ学習へ。
                // **一度決めたら二度は聞かない。**
                scope.launch {
                    val needsLearning = Prefs.groupBursts(context) &&
                        Learning.learned(context, project.id) == null
                    onCull(needsLearning)
                }
            },
            onDismiss = { starting = false }
        )
    }

    if (menu) {
        ModalBottomSheet(onDismissRequest = { menu = false }, containerColor = Surface) {
            Column(Modifier.padding(horizontal = 8.dp).padding(bottom = 24.dp)) {
                DetailMenuRow("写真を再読み込み") { menu = false; reloads += 1 }
                DetailMenuRow("選別を最初からやり直す", danger = true) {
                    menu = false; confirmRestart = true
                }
            }
        }
    }

    if (confirmRestart) {
        AlertDialog(
            onDismissRequest = { confirmRestart = false },
            title = { Text("選別を最初からやり直しますか") },
            // **何が消えるかを具体的に言う。**「よろしいですか」では判断できない。
            text = {
                Text(
                    "これまでに付けた星と、どこまで見たかが消えます。" +
                        "連写の手直しは残ります。写真そのものには手を触れません。"
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    confirmRestart = false
                    scope.launch { Store.clear(context, project.id); reloads += 1 }
                }) { Text("やり直す") }
            },
            dismissButton = {
                TextButton(onClick = { confirmRestart = false }) { Text("やめる") }
            }
        )
    }
}

/** 縦に溢れたら送れるようにする。Fold を畳んだときに下が切れないため。 */
@Composable
private fun Modifier.verticalScrollable(): Modifier =
    this.then(Modifier.verticalScroll(rememberScrollState()))

@Composable
private fun Card(content: @Composable ColumnScope.() -> Unit) {
    Column(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(14.dp))
            .background(Surface)
            .padding(16.dp),
        content = content
    )
}

@Composable
private fun PrepRow(label: String, done: Int, total: Int, complete: Boolean) {
    Row(Modifier.fillMaxWidth().padding(vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        Icon(
            if (complete) Icons.Filled.CheckCircle else Icons.Filled.Sync,
            null, Modifier.size(15.dp),
            tint = if (complete) Lime else Faint
        )
        Spacer(Modifier.width(8.dp))
        Text(label, fontSize = 13.sp, modifier = Modifier.weight(1f))
        Text("$done / $total", fontSize = 12.sp, color = Faint)
    }
    if (!complete && total > 0) {
        Box(
            Modifier.fillMaxWidth().height(3.dp).clip(RoundedCornerShape(2.dp))
                .background(Color(0xFF24272D))
        ) {
            Box(
                Modifier.fillMaxWidth(done.toFloat() / total).fillMaxHeight()
                    .clip(RoundedCornerShape(2.dp)).background(Lime)
            )
        }
    }
}

@Composable
private fun Chip(label: String, selected: Boolean, onClick: () -> Unit) {
    FilterChip(
        selected = selected,
        onClick = onClick,
        label = { Text(label, fontSize = 12.sp) },
        colors = FilterChipDefaults.filterChipColors(
            selectedContainerColor = Lime, selectedLabelColor = Color.Black
        )
    )
}

@Composable
private fun DetailMenuRow(label: String, danger: Boolean = false, onClick: () -> Unit) {
    Text(
        label,
        fontSize = 15.sp,
        color = if (danger) Color(0xFFFF8A80) else Color.White,
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(10.dp))
            .clickable(onClick = onClick)
            .padding(horizontal = 12.dp, vertical = 14.dp)
    )
}
