@file:OptIn(
    androidx.compose.material3.ExperimentalMaterial3Api::class,
    androidx.compose.foundation.ExperimentalFoundationApi::class
)

package app.photocurator.next

import android.content.Intent
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.IntentSenderRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Output
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
 * 選別結果。**星ごとに確かめて、取り出す。**
 *
 * 主ボタンは常に「いまの対象を…」。何に対して操作するのかを、
 * 押す前にボタンの文字だけで読み取れるようにする。
 */
@Composable
fun ResultsScreen(project: Project, star: Int, onBack: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    // 星の確認と取り出しの判断をする場所なので、選別と同じ絵を使う。
    val displayEdge = remember { Prefs.projectEdge(context, project.id) }

    var photos by remember { mutableStateOf<List<Photo>>(emptyList()) }
    // 選別の途中そのもの。**星を書き戻すので、抜き出した写しでは足りない。**
    var session by remember { mutableStateOf<Session?>(null) }
    var ratings by remember { mutableStateOf<Map<String, Int>>(emptyMap()) }
    var members by remember { mutableStateOf<Map<String, List<String>>>(emptyMap()) }
    // 星チップの選択。-1 で来たら「すべて」から始める。
    var filter by remember { mutableStateOf(if (star >= 0) "star:$star" else "all") }
    // 並べ替え。**既定は星が高い順**（結果を見に来る理由がそれ）。
    var sort by remember { mutableStateOf("star") }
    var picked by remember { mutableStateOf<Set<String>>(emptySet()) }
    var selecting by remember { mutableStateOf(false) }
    var note by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    var reloads by remember { mutableStateOf(0) }

    // 大きく見ている並びと、その何枚目か。**ここは見るだけ。**
    var zooming by remember { mutableStateOf<Pair<List<Photo>, Int>?>(null) }
    // 中身を選別している連写の代表。
    var reviewing by remember { mutableStateOf<String?>(null) }

    var outputMenu by remember { mutableStateOf(false) }
    var confirmFavourite by remember { mutableStateOf<List<Photo>?>(null) }
    var choosingDestination by remember { mutableStateOf(false) }
    var confirmingMove by remember { mutableStateOf<Album?>(null) }
    var pendingMove by remember { mutableStateOf<Pair<Album, List<Photo>>?>(null) }
    var pendingCount by remember { mutableStateOf(0) }

    // 取り出しの同意は OS の画面で取る。**戻ってきた結果を必ず言葉にする。**
    val consent = rememberLauncherForActivityResult(
        ActivityResultContracts.StartIntentSenderForResult()
    ) { result ->
        val move = pendingMove
        if (move == null) {
            note = Take.describe(result.resultCode, pendingCount)
            busy = false
            picked = emptySet(); selecting = false
            reloads += 1
        } else {
            pendingMove = null
            if (result.resultCode != android.app.Activity.RESULT_OK) {
                // **「失敗」と「やめた」を混ぜない。**
                note = "移していません（取り消されました）"
                busy = false
            } else {
                scope.launch {
                    val done = Take.move(context, move.second, move.first.relativeDir)
                    note = done.describe(move.first.name)
                    busy = false
                    picked = emptySet(); selecting = false
                    reloads += 1
                }
            }
        }
    }

    LaunchedEffect(project.id, reloads) {
        photos = Photos.forSource(context, project.source)
        val loaded = Store.load(context, project.id)
        session = loaded
        ratings = loaded?.ratings ?: emptyMap()
        members = loaded?.members ?: emptyMap()
        picked = emptySet()
        selecting = false
    }

    val counts = IntArray(6)
    for (photo in photos) counts[(ratings[photo.relativePath] ?: 0).coerceIn(0, 5)] += 1
    val atLeastOne = photos.count { (ratings[it.relativePath] ?: 0) > 0 }

    // 連写の仲間は代表に畳む。**同じ組が 5 枚並ぶと、何を見ればいいのか分からない。**
    // 中身は代表をタップして burst-review で見る。
    val folded = remember(photos, members) {
        val mates = members.entries
            .filter { it.value.size > 1 }
            .flatMap { entry -> entry.value.filter { it != entry.key } }
            .toSet()
        photos.filter { it.relativePath !in mates }
    }

    val picking = when {
        filter == "all" -> folded
        filter == "atLeast1" -> folded.filter { (ratings[it.relativePath] ?: 0) > 0 }
        else -> {
            val want = filter.removePrefix("star:").toIntOrNull() ?: 0
            folded.filter { (ratings[it.relativePath] ?: 0) == want }
        }
    }

    // **並べ替えは見せ方だけ。** 星も順番もここでは動かさない。
    val shown = remember(picking, sort, ratings) {
        if (sort == "time") picking.sortedWith(compareBy({ it.takenAt }, { it.relativePath }))
        else picking.sortedWith(
            compareByDescending<Photo> { ratings[it.relativePath] ?: 0 }
                .thenBy { it.takenAt }.thenBy { it.relativePath }
        )
    }
    val targets = if (picked.isEmpty()) shown else shown.filter { it.relativePath in picked }
    val targetLabel = if (picked.isEmpty()) {
        when {
            filter == "all" -> "すべて ${shown.size} 枚"
            filter == "atLeast1" -> "★1 以上 ${shown.size} 枚"
            else -> "${filter.removePrefix("star:").let { "★$it" }} ${shown.size} 枚"
        }
    } else "選んだ ${picked.size} 枚"

    // 連写の中身を選別。**選別画面と同じ部品**を使う別画面。
    reviewing?.let { head ->
        val order = photos.map { it.relativePath }
        val inside = (members[head] ?: listOf(head))
            .mapNotNull { path -> photos.firstOrNull { it.relativePath == path } }
            .sortedBy { order.indexOf(it.relativePath) }
        val base = ratings[head] ?: 0
        if (inside.size < 2) {
            reviewing = null
        } else {
            BurstReviewScreen(
                photos = inside,
                baseStar = base,
                displayEdge = displayEdge,
                onApply = { next ->
                    val current = session
                    if (current != null) {
                        // **まとまりはそのまま、星だけ動かす。**
                        val moved = current.copy(ratings = current.ratings + next)
                        session = moved
                        ratings = moved.ratings
                        scope.launch { Store.save(context, project.id, moved) }
                    }
                    reviewing = null
                },
                onZoom = { at -> zooming = inside to at },
                onBack = { reviewing = null }
            )
            return
        }
    }

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
        // ---- バー ----
        Row(
            Modifier.fillMaxWidth().height(64.dp).padding(end = 16.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            if (selecting) {
                IconButton(onClick = { selecting = false; picked = emptySet() }) {
                    Icon(Icons.Filled.Close, "選択をやめる")
                }
                Text("${picked.size} 枚を選択中", fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
                Spacer(Modifier.width(12.dp))
                TextButton(onClick = { picked = shown.map { it.relativePath }.toSet() }) {
                    Text("${shown.size} 枚すべて選択", fontSize = 12.sp)
                }
            } else {
                IconButton(onClick = onBack) { Icon(Icons.Filled.ArrowBack, "戻る") }
                Text("結果 · ${project.name}", fontSize = 16.sp, fontWeight = FontWeight.SemiBold)
            }
            Spacer(Modifier.weight(1f))
            Button(
                onClick = { outputMenu = true },
                enabled = !busy && targets.isNotEmpty(),
                shape = RoundedCornerShape(50)
            ) {
                Icon(Icons.Filled.Output, null, Modifier.size(16.dp))
                Spacer(Modifier.width(6.dp))
                Text("$targetLabel を…", fontWeight = FontWeight.Bold, fontSize = 13.sp)
            }
        }

        Column(Modifier.padding(horizontal = 16.dp)) {
            // ---- 星チップ ----
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                StarChip("すべて ${photos.size}", filter == "all") { filter = "all"; picked = emptySet() }
                if (atLeastOne > 0) {
                    StarChip("★1 以上 $atLeastOne", filter == "atLeast1") {
                        filter = "atLeast1"; picked = emptySet()
                    }
                }
                for (value in 5 downTo 0) {
                    if (counts[value] == 0) continue
                    StarChip("★$value · ${counts[value]}", filter == "star:$value") {
                        filter = "star:$value"; picked = emptySet()
                    }
                }
                Spacer(Modifier.weight(1f))
                // **並べ替えは 2 つだけ。** 星で選んだのか、撮った順で見たいのか。
                StarChip("星が高い順", sort == "star") { sort = "star" }
                StarChip("撮影順", sort == "time") { sort = "time" }
            }

            // ---- 星の内訳バー ----
            Spacer(Modifier.height(10.dp))
            Row(Modifier.fillMaxWidth().height(6.dp).clip(RoundedCornerShape(3.dp))) {
                for (value in 5 downTo 0) {
                    if (counts[value] == 0) continue
                    Box(
                        Modifier.weight(counts[value].toFloat()).fillMaxHeight()
                            .background(
                                if (value > 0) Lime.copy(alpha = 0.35f + value * 0.13f)
                                else Color(0xFF2C3038)
                            )
                    )
                }
            }

            note?.let {
                Text(it, fontSize = 12.sp, color = Faint, modifier = Modifier.padding(top = 8.dp))
            }
        }

        Spacer(Modifier.height(10.dp))

        if (shown.isEmpty()) {
            Box(Modifier.weight(1f), contentAlignment = Alignment.Center) {
                Text("この星の写真はありません", color = Faint, fontSize = 13.sp)
            }
        } else {
            LazyVerticalGrid(
                columns = GridCells.Adaptive(minSize = 132.dp),
                modifier = Modifier.weight(1f),
                contentPadding = PaddingValues(horizontal = 14.dp)
            ) {
                items(shown, key = { it.id }) { photo ->
                    val on = photo.relativePath in picked
                    val burst = members[photo.relativePath]?.size ?: 1
                    Box(
                        Modifier
                            .padding(2.dp)
                            .aspectRatio(1f)
                            .clip(RoundedCornerShape(6.dp))
                            .background(Tile)
                            .then(
                                when {
                                    on -> Modifier.border(3.dp, Lime, RoundedCornerShape(6.dp))
                                    burst > 1 -> Modifier.border(1.dp, Color.White, RoundedCornerShape(6.dp))
                                    else -> Modifier
                                }
                            )
                            .combinedClickable(
                                onClick = {
                                    if (selecting) {
                                        picked = if (on) picked - photo.relativePath
                                        else picked + photo.relativePath
                                    } else if (burst > 1) {
                                        // **連写の組は中身を選別できる。**
                                        // 代表が通ると仲間も同じ星になるので、
                                        // その差をつけ直す場所がここにしかない。
                                        reviewing = photo.relativePath
                                    } else {
                                        // **選んでいないときのタップは拡大。**
                                        // ここは見るだけなので「残す」は出さない。
                                        zooming = shown to shown.indexOf(photo)
                                    }
                                },
                                // **長押しで選択に入る。** 普段のタップは選択にしない。
                                onLongClick = {
                                    selecting = true
                                    picked = picked + photo.relativePath
                                }
                            )
                    ) {
                        val format = remember(photo.id) { unsupportedFormat(photo.name) }
                        var state by remember(photo.id) {
                            mutableStateOf(
                                if (format != null) Preview.Unsupported else Preview.Generating
                            )
                        }
                        EmptyTile(state, format)
                        if (state != Preview.Unsupported) {
                            AsyncImage(
                                model = ImageRequest.Builder(LocalContext.current)
                                    .data(photo.displayModel(displayEdge)).size(400).build(),
                                contentDescription = photo.name,
                                imageLoader = Images.loader(LocalContext.current),
                                contentScale = ContentScale.Crop,
                                onSuccess = { state = Preview.Ready },
                                onError = { state = Preview.Failed },
                                modifier = Modifier.fillMaxSize()
                            )
                        }
                        val value = ratings[photo.relativePath] ?: 0
                        if (value > 0) {
                            Text(
                                "★$value",
                                fontSize = 11.sp, color = Lime,
                                modifier = Modifier
                                    .align(Alignment.BottomStart)
                                    .padding(5.dp)
                                    .clip(RoundedCornerShape(4.dp))
                                    .background(Color(0xB3101114))
                                    .padding(horizontal = 5.dp, vertical = 1.dp)
                            )
                        }
                        if (burst > 1) {
                            Text(
                                "⧉$burst",
                                fontSize = 11.sp, color = Color.White,
                                modifier = Modifier
                                    .align(Alignment.TopStart)
                                    .padding(5.dp)
                                    .clip(RoundedCornerShape(4.dp))
                                    .background(Color(0xB3101114))
                                    .padding(horizontal = 5.dp, vertical = 1.dp)
                            )
                        }
                        if (on) {
                            Box(
                                Modifier.align(Alignment.TopEnd).padding(4.dp)
                                    .clip(RoundedCornerShape(50)).background(Lime).padding(2.dp)
                            ) {
                                Icon(Icons.Filled.Check, null, Modifier.size(14.dp), tint = Color.Black)
                            }
                        }
                    }
                }
            }
        }

        // 将来の同期状態の席。**いまは端末だけだと言い切る。**
        Text(
            "この端末だけの結果（NAS との同期は今後）",
            fontSize = 11.sp, color = Faint,
            modifier = Modifier.padding(16.dp)
        )
    }

    // ---- 「[対象] を…」メニュー ----
    if (outputMenu) {
        ModalBottomSheet(onDismissRequest = { outputMenu = false }, containerColor = Surface) {
            Column(Modifier.padding(horizontal = 8.dp).padding(bottom = 24.dp)) {
                Text(
                    "$targetLabel を…",
                    fontSize = 15.sp, fontWeight = FontWeight.SemiBold,
                    modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp)
                )
                OutputRow("アルバムに移動", "原本を動かします") {
                    outputMenu = false; choosingDestination = true
                }
                OutputRow("お気に入りに追加", "原本の属性を変えます") {
                    outputMenu = false; confirmFavourite = targets
                }
                OutputRow("共有", "端末の共有メニューへ") {
                    outputMenu = false
                    val send = Intent(Intent.ACTION_SEND_MULTIPLE).apply {
                        type = "image/*"
                        putParcelableArrayListExtra(
                            Intent.EXTRA_STREAM,
                            ArrayList(targets.map { it.uri })
                        )
                        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                    }
                    context.startActivity(Intent.createChooser(send, "共有"))
                }
            }
        }
    }

    // **写真そのものに触る前に、必ずここを通す。**
    //
    // OS の同意画面は、いつも出るとは限らない。すでにその写真への許可を
    // 持っていると MediaProvider は黙って通す。実機で確認したとき、
    // 画面が出ないまま 4 枚に印が付いた。
    confirmFavourite?.let { list ->
        ConfirmDialog(
            title = "${list.size} 枚にお気に入りを付けます",
            body = "端末の写真に印が付きます（Google フォトなどからも見えます）。" +
                "写真そのものは動かしませんし、消えません。後から外せます。",
            confirmLabel = "付ける",
            touchesOriginals = true,
            onConfirm = {
                confirmFavourite = null
                val sender = Take.favouriteRequest(context, list)
                if (sender == null) {
                    note = "この端末ではお気に入りを付けられません"
                } else {
                    pendingCount = list.size
                    busy = true
                    consent.launch(IntentSenderRequest.Builder(sender).build())
                }
            },
            onDismiss = { confirmFavourite = null }
        )
    }

    if (choosingDestination) {
        DestinationSheet(
            from = project,
            count = targets.size,
            onPick = { destination -> choosingDestination = false; confirmingMove = destination },
            onDismiss = { choosingDestination = false }
        )
    }

    // **移すのは戻しにくい。** お気に入りより強い言い方で確かめる。
    confirmingMove?.let { destination ->
        ConfirmDialog(
            title = "${targets.size} 枚を「${destination.name}」へ移します",
            body = "端末の中の置き場所が ${destination.relativeDir} に変わります。" +
                "写真は消えませんが、元のアルバムからは無くなります。" +
                "この操作にアプリ側の取り消しはありません。",
            confirmLabel = "移す",
            touchesOriginals = true,
            onConfirm = {
                val list = targets
                confirmingMove = null
                val sender = Take.moveRequest(context, list)
                if (sender == null) {
                    note = "この端末では移せません"
                } else {
                    pendingMove = destination to list
                    busy = true
                    consent.launch(IntentSenderRequest.Builder(sender).build())
                }
            },
            onDismiss = { confirmingMove = null }
        )
    }
}

@Composable
private fun StarChip(label: String, selected: Boolean, onClick: () -> Unit) {
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
private fun OutputRow(label: String, note: String, onClick: () -> Unit) {
    Column(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(10.dp))
            .clickable(onClick = onClick)
            .padding(horizontal = 12.dp, vertical = 12.dp)
    ) {
        Text(label, fontSize = 15.sp)
        // **原本に触るものは、押す前にそう書く。**
        Text(note, fontSize = 11.sp, color = Faint)
    }
}
