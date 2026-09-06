package app.photocurator.next

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.MoreHoriz
import androidx.compose.material.icons.filled.Star
import androidx.compose.material.icons.filled.Undo
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import coil.compose.AsyncImage
import coil.request.ImageRequest
import kotlinx.coroutines.launch
import uniffi.photo_curator_core.BurstThreshold
import uniffi.photo_curator_core.PhotoRef
import uniffi.photo_curator_core.Session
import uniffi.photo_curator_core.advance
import uniffi.photo_curator_core.startRound
import uniffi.photo_curator_core.undo

/**
 * 選別画面。**このアプリの本体。**
 *
 * 帯は上の 1 本だけ。残りは全部写真に使う。判断（誰が通るか、星がどう動くか）
 * は core（Rust）が持ち、ここは見せることと触らせることだけをする。
 */

/** 枚数と表示領域の縦横比から、枠の行×列を決める。 */
private val GRID = mapOf(
    1 to (1 to 1), 2 to (1 to 2), 3 to (1 to 3), 4 to (2 to 2), 5 to (2 to 3),
    6 to (2 to 3), 7 to (2 to 4), 8 to (2 to 4), 9 to (3 to 3), 10 to (2 to 5)
)

private fun gridFor(count: Int, landscape: Boolean): Pair<Int, Int> {
    val (rows, cols) = GRID[count] ?: ((count + 4) / 5 to minOf(5, maxOf(1, count)))
    return if (landscape) rows to cols else cols to rows
}

@Composable
fun CullScreen(album: Album, onBack: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()

    var photos by remember { mutableStateOf<List<Photo>>(emptyList()) }
    var session by remember { mutableStateOf<Session?>(null) }
    var selected by remember { mutableStateOf<Set<String>>(emptySet()) }
    var multi by remember { mutableStateOf(false) }
    var note by remember { mutableStateOf("読み込み中…") }
    var stageSize by remember { mutableStateOf(0 to 0) }

    // 相対パスから写真を引く。core は相対パスしか知らない。
    val byPath = remember(photos) { photos.associateBy { it.relativePath } }

    LaunchedEffect(album.id) {
        photos = Photos.photos(context, album.id)
        // **途中があれば続きから。** 無ければ新しく始める。
        val saved = Store.load(context, album.id)
        session = saved ?: startRound(
            photos.map {
                PhotoRef(
                    relativePath = it.relativePath,
                    capturedAt = it.takenAt,
                    // dHash はまだ作っていない。いまは時間だけで切れる。
                    dHash = null,
                    dHashVersion = 2
                )
            },
            groupSize = 4u,
            targetStar = 0,
            groupBursts = true,
            threshold = BurstThreshold(windowMs = 4000, distance = 6u, dHashVersion = 2),
            overrides = emptyList()
        )
        note = if (saved != null) "続きから" else ""
    }

    val live = session
    if (live == null) {
        Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
            Text(note, color = Faint)
        }
        return
    }

    /** 確定して次へ。**確定のたびに保存する。どこで止めても失わない。** */
    fun commit(picked: Set<String>) {
        val next = advance(live, picked.toList())
        session = next
        selected = emptySet()
        multi = false
        scope.launch { Store.save(context, album.id, next) }
    }

    fun stepBack() {
        val next = undo(live)
        session = next
        selected = emptySet()
        scope.launch { Store.save(context, album.id, next) }
    }

    Column(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing)) {
        CullBar(
            album = album,
            session = live,
            selectedCount = selected.size,
            multi = multi,
            onBack = onBack,
            onUndo = { stepBack() },
            onToggleMulti = { multi = !multi; selected = emptySet() },
            onClear = { selected = emptySet() },
            onCommit = { commit(selected) }
        )

        // 進捗は 2px の線 1 本。数字はバーの中央 1 か所だけ。
        val seen = live.history.size
        val total = seen + live.queue.size / maxOf(1, live.groupSize.toInt()) + 1
        LinearProgressIndicator(
            progress = { if (live.finished) 1f else seen.toFloat() / maxOf(1, total) },
            modifier = Modifier.fillMaxWidth().height(2.dp),
            color = Lime, trackColor = Color(0xFF24272D), gapSize = 0.dp, drawStopIndicator = {}
        )

        if (live.finished) {
            RoundDone(live, onBack)
            return@Column
        }

        val landscape = stageSize.first >= stageSize.second
        val (rows, cols) = gridFor(live.current.size, landscape)

        Column(
            Modifier
                .weight(1f)
                .padding(6.dp)
                .onSizeChanged { stageSize = it.width to it.height }
        ) {
            for (row in 0 until rows) {
                Row(Modifier.weight(1f).fillMaxWidth()) {
                    for (col in 0 until cols) {
                        val at = row * cols + col
                        Box(Modifier.weight(1f).fillMaxHeight().padding(3.dp)) {
                            live.current.getOrNull(at)?.let { path ->
                                Tile(
                                    number = at + 1,
                                    photo = byPath[path],
                                    picked = path in selected,
                                    onTap = {
                                        if (multi) {
                                            selected = if (path in selected) selected - path
                                            else selected + path
                                        } else {
                                            // **単数では押した瞬間に確定して次へ。**
                                            // 1 タップ増やさないのがこの画面の要点。
                                            commit(setOf(path))
                                        }
                                    }
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun CullBar(
    album: Album,
    session: Session,
    selectedCount: Int,
    multi: Boolean,
    onBack: () -> Unit,
    onUndo: () -> Unit,
    onToggleMulti: () -> Unit,
    onClear: () -> Unit,
    onCommit: () -> Unit
) {
    Row(
        Modifier.fillMaxWidth().height(56.dp).padding(horizontal = 4.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        IconButton(onClick = onBack) { Icon(Icons.Filled.ArrowBack, "中断して戻る") }
        IconButton(onClick = onUndo, enabled = session.history.isNotEmpty()) {
            Icon(Icons.Filled.Undo, "1 つ戻す")
        }
        FilterChip(
            selected = multi, onClick = onToggleMulti,
            label = { Text("複数", fontSize = 12.sp) },
            leadingIcon = if (multi) {
                { Icon(Icons.Filled.Check, null, Modifier.size(16.dp)) }
            } else null
        )
        if (multi && selectedCount > 0) {
            TextButton(onClick = onClear) { Text("解除", fontSize = 12.sp) }
        }

        Column(Modifier.weight(1f), horizontalAlignment = Alignment.CenterHorizontally) {
            Text(
                "★${session.targetStar} を選別中 · ROUND ${session.round}",
                fontSize = 11.sp, color = Lime
            )
            Text(
                "残り ${session.queue.size + session.current.size} 枚",
                fontSize = 13.sp
            )
        }

        Button(
            onClick = onCommit,
            shape = androidx.compose.foundation.shape.RoundedCornerShape(50),
            enabled = !session.finished
        ) {
            // **結果を枚数で言う。** 押すと何が起きるかが読み取れるように。
            Text(
                if (selectedCount == 0) "${session.current.size} 枚とも落とす"
                else "$selectedCount 枚を残す",
                fontWeight = FontWeight.Bold, fontSize = 13.sp
            )
        }
    }
}

@Composable
private fun Tile(number: Int, photo: Photo?, picked: Boolean, onTap: () -> Unit) {
    Box(
        Modifier
            .fillMaxSize()
            .clip(androidx.compose.foundation.shape.RoundedCornerShape(8.dp))
            .background(Tile)
            .then(if (picked) Modifier.border(3.dp, Lime) else Modifier)
            .clickable(enabled = photo != null, onClick = onTap)
    ) {
        if (photo == null) {
            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                Text("読めません", fontSize = 11.sp, color = Faint)
            }
        } else {
            AsyncImage(
                model = ImageRequest.Builder(LocalContext.current)
                    .data(photo.uri)
                    // **要求した大きさでデコードする。** 原本を丸ごと載せない。
                    .size(1280)
                    .build(),
                contentDescription = photo.name,
                // 切らずに全部見せる。縦横比が合わなくても黒帯にしない。
                contentScale = ContentScale.Fit,
                modifier = Modifier.fillMaxSize()
            )
        }
        Box(
            Modifier
                .padding(6.dp)
                .clip(androidx.compose.foundation.shape.RoundedCornerShape(6.dp))
                .background(if (picked) Lime else Color(0xB3101114))
                .padding(horizontal = 7.dp, vertical = 2.dp)
        ) {
            Text(
                if (number == 10) "0" else "$number",
                fontSize = 12.sp, fontWeight = FontWeight.SemiBold,
                color = if (picked) Color.Black else Color.White
            )
        }
    }
}

/** ラウンドの終わり。**1 行目で何枚残ったかを答える。** */
@Composable
private fun RoundDone(session: Session, onBack: () -> Unit) {
    val kept = session.survivors.size
    val seen = session.history.sumOf { it.group.size }
    Column(
        Modifier.fillMaxSize().padding(24.dp),
        verticalArrangement = Arrangement.Center
    ) {
        Text("ROUND ${session.round} 完了", fontSize = 12.sp, color = Lime)
        Text(
            "★${session.targetStar + 1} が $kept 枚残りました",
            fontSize = 22.sp, fontWeight = FontWeight.Bold
        )
        Text(
            "$seen 枚から $kept 枚に絞られました。",
            fontSize = 13.sp, color = Faint, modifier = Modifier.padding(top = 6.dp)
        )
        Spacer(Modifier.height(24.dp))
        Button(onClick = onBack, shape = androidx.compose.foundation.shape.RoundedCornerShape(50)) {
            Text("アルバムへ戻る", fontWeight = FontWeight.Bold)
        }
    }
}
