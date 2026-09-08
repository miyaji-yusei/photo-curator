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
    onResults: () -> Unit,
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
    // 「写真を再読み込み」を押されたか。**押されたときだけ網へ行く。**
    var rescan by remember { mutableStateOf(false) }
    // 開始前の確認を出しているか。**初回は必ず出す。**
    var starting by remember { mutableStateOf(false) }
    // 準備でつまずいたこと。（人の言葉, 技術文言）。**詳細は開いたときだけ出す。**
    var trouble by remember { mutableStateOf<Pair<String, String>?>(null) }
    var troubleDetail by remember { mutableStateOf(false) }
    // 大きく見ている並びと、その何枚目か。**ここは見るだけ**なので星は動かない。
    var zooming by remember { mutableStateOf<Pair<List<Photo>, Int>?>(null) }

    // **開いたら準備が動き出す。** カードに数が出ているのに何も進まないと、
    // 止まっているのか終わっているのか分からない。
    var preparing by remember { mutableStateOf(0 to 0) }
    // 表示用画像の進み。**NAS のときだけ動く。**
    var rendering by remember { mutableStateOf(0 to 0) }
    var displayEdge by remember { mutableStateOf(Prefs.projectEdge(context, project.id)) }
    // 「…」から大きさを選んでいるか。
    var choosingEdge by remember { mutableStateOf(false) }

    LaunchedEffect(project.id, reloads) {
        scanned = false
        trouble = null
        session = Store.load(context, project.id)
        prints = Fingerprints.load(context, project.source.key)
        // **転んだままなら、まずそれを出す。** 準備をやり直すのは押されたとき。
        val noted = Trouble.load(context, project.source.key)
        photos = Listing.load(context, project.source.key)
            ?: if (noted != null) emptyList() else Photos.forSource(context, project.source)
        scanned = true
        try {
            // 走査が終わってから指紋。**できた分から選別に出せる。**
            val ready = Prepare.run(context, project, rescan) { done, total ->
                preparing = done to total
            }
            rescan = false
            photos = ready.first
            prints = Fingerprints.load(context, project.source.key)

            // 3 段目。**原本を読むのはここだけ。** できた分から選別に出せる。
            if (project.source.kind == "nas") {
                Prepare.renders(context, project, ready.first, displayEdge) { done, total ->
                    rendering = done to total
                }
            }
            // 通ったら**前の転びは消す**。古い赤字を出し続けない。
            Trouble.clear(context, project.source.key)
        } catch (error: Exception) {
            // **黙って落とさない。** 何が起きたかを 1 文にして、ホームにも残す。
            val said = Smb.describe(error)
            trouble = said to (error.message ?: error.javaClass.name)
            Trouble.note(context, project.source.key, said)
            rescan = false
        }
    }

    val live = session
    val ratings = live?.ratings ?: emptyMap()
    val starred = photos.count { (ratings[it.relativePath] ?: 0) > 0 }
    val bursts = live?.members?.size ?: 0

    // 拡大は画面を覆う。**開いているあいだ下は組まない。**
    zooming?.let { (line, index) ->
        ZoomView(
            photos = line,
            startAt = index,
            displayEdge = displayEdge,
            onClose = { zooming = null }
        )
        return
    }

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
                    when {
                        // **終わっているなら結果へ直行。** 見るだけなのに
                        // 「選別を始める前に」を挟むのは筋が通らない。
                        live?.finished == true -> onResults()
                        // 初回か、「毎回確認」が on のときだけ挟む。
                        live == null || Prefs.askBeforeStart(context) -> starting = true
                        else -> onCull(false)
                    }
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

        // ---- 左: 出所 / 準備 / 選別 ----
        val cards: @Composable () -> Unit = {
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
                        // **いつ撮ったものかを 1 行で。** 同じ名前のフォルダが
                        // 並んだとき、枚数だけでは見分けがつかない。
                        "${photos.size} 枚" + span(photos)?.let { " · $it" }.orEmpty(),
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
                            when {
                                trouble != null -> "止まっています"
                                scanned && prints.size >= photos.size &&
                                    (project.source.kind != "nas" ||
                                        (rendering.second > 0 && rendering.first >= rendering.second))
                                -> "完了"
                                else -> "進行中"
                            },
                            fontSize = 12.sp, color = if (trouble != null) Warn else Lime
                        )
                    }

                    // **転んだ理由はその場に出す。** 上に赤い帯は出さない。
                    trouble?.let { (said, technical) ->
                        Spacer(Modifier.height(10.dp))
                        Text(said, fontSize = 13.sp, color = Warn)
                        Text(
                            if (project.source.kind == "nas")
                                "同じ Wi-Fi につながっているか、NAS の電源を確かめてください"
                            else "写真へのアクセスが許可されているか確かめてください",
                            fontSize = 12.sp, color = Faint,
                            modifier = Modifier.padding(top = 2.dp)
                        )
                        Row(
                            Modifier.padding(top = 8.dp),
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            OutlinedButton(
                                onClick = { trouble = null; reloads += 1 },
                                shape = RoundedCornerShape(50)
                            ) { Text("再試行", fontSize = 12.sp) }
                            Spacer(Modifier.width(8.dp))
                            TextButton(onClick = { troubleDetail = !troubleDetail }) {
                                Text(if (troubleDetail) "詳細を閉じる" else "詳細", fontSize = 12.sp)
                            }
                        }
                        if (troubleDetail) {
                            // **技術文言は開いたときだけ。** 普段は人の言葉 1 文で足りる。
                            Text(
                                technical,
                                fontSize = 11.sp, color = Faint,
                                fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace,
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .clip(RoundedCornerShape(8.dp))
                                    .background(Tile)
                                    .padding(10.dp)
                            )
                        }
                    }
                    Spacer(Modifier.height(10.dp))
                    PrepRow("写真の走査", if (scanned) photos.size else 0, photos.size, scanned)
                    // Tauri 版の「表示用画像」に当たる段。ネイティブでは OS の縮小画像を
                    // そのまま使うので、実際に作るのは連写のまとめに使う指紋だけ。
                    // 指紋は撮影時刻と同じ 1 回の読みで取れるので、同じ行に畳む。
                    PrepRow(
                        "撮影時刻・サムネイル",
                        maxOf(prints.size, preparing.first),
                        photos.size,
                        prints.size >= photos.size && photos.isNotEmpty()
                    )
                    // **NAS のときだけ。** 端末の写真は手元でデコードすれば足りる。
                    if (project.source.kind == "nas") {
                        PrepRow(
                            "表示用画像（${displayEdge}px）",
                            rendering.first,
                            photos.size,
                            rendering.second > 0 && rendering.first >= rendering.second
                        )
                    }
                    Text(
                        if (project.source.kind == "nas")
                            "できた写真から選別に出ます。NAS から読むので Wi-Fi 推奨"
                        else "できた写真から選別に出ます",
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

        // ---- 右: 写真の一覧。走査と準備の結果を確かめる場所 ----
        val gallery: @Composable () -> Unit = {
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
                        // **なぜ絵が無いのかを、タイルの中で言う。**
                        // 読めない形式・まだ作っていない・作れなかった、を分ける。
                        val format = remember(photo.id) { unsupportedFormat(photo.name) }
                        var state by remember(photo.id) {
                            mutableStateOf(
                                when {
                                    format != null -> Preview.Unsupported
                                    // 指紋が出来ていれば、その 1 回の読みで
                                    // サムネイルも取れている。
                                    prints.containsKey(photo.relativePath) -> Preview.Ready
                                    preparing.second > 0 -> Preview.Generating
                                    else -> Preview.Queued
                                }
                            )
                        }
                        Box(
                            Modifier
                                .aspectRatio(1f)
                                .clip(RoundedCornerShape(6.dp))
                                .background(Tile)
                                .clickable {
                                    // **絞った並びのまま前後へ送れる。**
                                    zooming = shown to shown.indexOf(photo)
                                }
                        ) {
                            EmptyTile(state, format)
                            if (state != Preview.Unsupported) {
                                AsyncImage(
                                    model = ImageRequest.Builder(LocalContext.current)
                                        .data(photo.thumbModel).size(256).build(),
                                    contentDescription = photo.name,
                                    imageLoader = Images.loader(LocalContext.current),
                                    contentScale = ContentScale.Crop,
                                    onSuccess = { state = Preview.Ready },
                                    // **読めなかったことは黙らない。**
                                    onError = { state = Preview.Failed },
                                    modifier = Modifier.fillMaxSize()
                                )
                            }
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

        // **<600dp（Fold のカバー）では横に 2 つ置けない。** 縦に積む。
        // 設計の「<600 は縦積み」。写真の面積を最優先にして、上のカードは
        // 高さを抑え、そこだけ中で送れるようにする。
        BoxWithConstraints(Modifier.fillMaxSize().padding(horizontal = 16.dp)) {
            if (maxWidth < 600.dp) {
                Column(Modifier.fillMaxSize()) {
                    Column(Modifier.heightIn(max = 300.dp).verticalScrollable()) { cards() }
                    Spacer(Modifier.height(12.dp))
                    Column(Modifier.weight(1f)) { gallery() }
                }
            } else {
                Row(Modifier.fillMaxSize()) {
                    Column(Modifier.width(340.dp).verticalScrollable()) { cards() }
                    Spacer(Modifier.width(16.dp))
                    Column(Modifier.weight(1f)) { gallery() }
                }
            }
        }
    }

    if (starting) {
        StartSheet(
            project = project,
            photoCount = photos.size,
            readyCount = if (project.source.kind == "nas") rendering.first else photos.size,
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
                DetailMenuRow("写真を再読み込み") { menu = false; rescan = true; reloads += 1 }
                if (project.source.kind == "nas") {
                    DetailMenuRow("表示用画像の大きさ（${displayEdge}px）") {
                        menu = false; choosingEdge = true
                    }
                }
                DetailMenuRow("選別を最初からやり直す", danger = true) {
                    menu = false; confirmRestart = true
                }
            }
        }
    }

    if (choosingEdge) {
        ModalBottomSheet(onDismissRequest = { choosingEdge = false }, containerColor = Surface) {
            Column(Modifier.padding(horizontal = 20.dp).padding(bottom = 24.dp)) {
                Text("表示用画像の大きさ", fontSize = 16.sp, fontWeight = FontWeight.SemiBold)
                Text(
                    // **作り直しになることを先に言う。** 押してから 20 分待たせない。
                    "このプロジェクトだけに効きます。まだ作っていない大きさは、" +
                        "選んだあとに作り直します",
                    fontSize = 12.sp, color = Faint,
                    modifier = Modifier.padding(top = 4.dp, bottom = 12.dp)
                )
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    for (edge in listOf(768, 1024, 1280, 1536, 1920)) {
                        Chip("${edge}px", edge == displayEdge) {
                            displayEdge = edge
                            Prefs.setProjectEdge(context, project.id, edge)
                            choosingEdge = false
                            rendering = 0 to 0
                            reloads += 1
                        }
                    }
                }
            }
        }
    }

    if (confirmRestart) {
        ConfirmDialog(
            title = "選別を最初からやり直しますか",
            // **何が消えるかを具体的に言う。**「よろしいですか」では判断できない。
            body = "これまでに付けた星と、どこまで見たかが消えます。" +
                "連写のまとめ方（最初に答えた基準と、手で直した分）も消えるので、" +
                "次に始めるときはもう一度「同じ連写ですか」から聞きます。" +
                "写真そのものには手を触れません。",
            confirmLabel = "やり直す",
            // 原本には触らないので赤字は出さない。**赤を安売りしない。**
            onConfirm = {
                confirmRestart = false
                scope.launch {
                    Store.clear(context, project.id)
                    // **基準と手直しも消す。** ここを残すと、やり直しても
                    // 同じまとめ方になり、聞き直す道も無くなる。
                    Learning.forget(context, project.id)
                    Overrides.clear(context, project.id)
                    reloads += 1
                }
            },
            onDismiss = { confirmRestart = false }
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

/** 「2021/06/13 13:02 – 17:48」。**同じ日なら日付は 1 回だけ。** */
private fun span(photos: List<Photo>): String? {
    val times = photos.map { it.takenAt }.filter { it > 0 }
    if (times.isEmpty()) return null
    val day = java.text.SimpleDateFormat("yyyy/MM/dd HH:mm", java.util.Locale.JAPAN)
    val clock = java.text.SimpleDateFormat("HH:mm", java.util.Locale.JAPAN)
    val date = java.text.SimpleDateFormat("yyyy/MM/dd", java.util.Locale.JAPAN)
    val from = java.util.Date(times.min())
    val to = java.util.Date(times.max())
    return if (date.format(from) == date.format(to)) {
        "${day.format(from)} – ${clock.format(to)}"
    } else {
        "${day.format(from)} – ${day.format(to)}"
    }
}
