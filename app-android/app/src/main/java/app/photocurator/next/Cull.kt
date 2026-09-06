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
import uniffi.photo_curator_core.nextRound
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
    // core に渡す形。**次のラウンドでも同じものを使う**ので持っておく。
    var refs by remember { mutableStateOf<List<PhotoRef>>(emptyList()) }
    var session by remember { mutableStateOf<Session?>(null) }
    var selected by remember { mutableStateOf<Set<String>>(emptySet()) }
    var multi by remember { mutableStateOf(false) }
    var note by remember { mutableStateOf("読み込み中…") }
    var stageSize by remember { mutableStateOf(0 to 0) }
    // 連写のまとめに使う指紋。**出来た分だけで始められる。**
    var prepared by remember { mutableStateOf(0 to 0) }

    // 相対パスから写真を引く。core は相対パスしか知らない。
    val byPath = remember(photos) { photos.associateBy { it.relativePath } }

    /**
     * まとめる基準。**端末で測って決めた値。**
     *
     * Camera（694 枚）の 4 秒以内に並ぶ 173 組を測ると、距離は二山になった:
     * 0-9 に 28 組（連写）、15 以上に 130 組（たまたま近い時刻に入った別の絵）。
     * 10-14 はどちらとも言えない 15 組。指紋そのものの揺れは 1。
     *
     * 9 を採るのは、**間違えて繋ぐ方が高くつく**から。繋いでしまうと片方は
     * 二度と画面に出ず、選んだ覚えのない星が付く。切りすぎたときは
     * 両方見えるだけで、気付けるし直せる。
     */
    val threshold = remember {
        BurstThreshold(windowMs = 4000, distance = 9u, dHashVersion = Analyse.VERSION)
    }

    LaunchedEffect(album.id) {
        note = "写真を読み込んでいます…"
        photos = Photos.photos(context, album.id)

        // 指紋を作る。OS の縮小画像から作るので**原本を読まない**（1 枚 3.5ms）。
        // **前に作った分は作り直さない。** 開くたびに解析し直すと、
        // 何が起きているのか誰にも分からなくなる。
        note = "似た写真を調べています…"
        val cached = Fingerprints.load(context, album.id)
        val prints = Analyse.fingerprints(context, photos, cached) { done, total ->
            prepared = done to total
        }
        // 変わっていなければ書かない。書く回数はそのまま壊れる機会になる。
        if (prints != cached) Fingerprints.save(context, album.id, prints)

        refs = photos.map {
            PhotoRef(
                relativePath = it.relativePath,
                capturedAt = it.takenAt,
                // **作れなかったものは null のまま。** 0 を入れると
                // 読めない写真どうしが同一に見えて誤ってまとまる。
                dHash = prints[it.relativePath]?.hash,
                dHashVersion = Analyse.VERSION
            )
        }
        photos.firstOrNull()?.let { Analyse.selfCheck(context, it) }
        Neighbours.log(refs, threshold)

        // **途中があれば続きから。** 無ければ新しく始める。
        val saved = Store.load(context, album.id)
        session = saved ?: startRound(
            refs,
            groupSize = 4u,
            targetStar = 0,
            groupBursts = true,
            threshold = threshold,
            overrides = emptyList()
        )
        note = if (saved != null) "続きから" else ""
    }

    val live = session
    if (live == null) {
        Column(
            Modifier.fillMaxSize(), verticalArrangement = Arrangement.Center,
            horizontalAlignment = Alignment.CenterHorizontally
        ) {
            Text(note, color = Faint, fontSize = 13.sp)
            // **数が分かるものは done / total で出す。** 終わらないバーは出さない。
            if (prepared.second > 0) {
                Text(
                    "${prepared.first} / ${prepared.second}",
                    color = Lime, fontSize = 18.sp, fontWeight = FontWeight.Bold,
                    modifier = Modifier.padding(top = 8.dp)
                )
            }
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
            // **進めるかどうかを、聞かれる前に確かめておく。**
            // 「次へ」を出しておいて何も起きない画面が、一番信用を失う。
            val upcoming = remember(live, refs) {
                nextRound(live, refs, true, threshold, emptyList())
            }
            RoundDone(
                session = live,
                upcoming = upcoming,
                onNext = { next ->
                    session = next
                    selected = emptySet()
                    scope.launch { Store.save(context, album.id, next) }
                },
                onBack = onBack
            )
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
        // ラウンドが終わったら、選別のための道具は出さない。**押せるものは効くもの
        // だけにする。** 効かないボタンが並ぶと、他のボタンまで信用されなくなる。
        if (!session.finished) {
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
        }

        Column(Modifier.weight(1f), horizontalAlignment = Alignment.CenterHorizontally) {
            if (session.finished) {
                Text(album.name, fontSize = 13.sp)
            } else {
                Text(
                    "★${session.targetStar} を選別中 · ROUND ${session.round}",
                    fontSize = 11.sp, color = Lime
                )
                // **畳んだことを隠さない。** 人は枚数で考えているので、
                // 代表の数だけを「残り」と言うと数が合わなくて不安になる。
                val remainingGroups = session.queue.size + session.current.size
                val remainingPhotos = (session.queue + session.current)
                    .sumOf { session.members[it]?.size ?: 1 }
                Text(
                    if (remainingPhotos == remainingGroups) "残り $remainingGroups 枚"
                    else "残り $remainingGroups 組 · $remainingPhotos 枚",
                    fontSize = 13.sp
                )
            }
        }

        if (!session.finished) {
            Button(
                onClick = onCommit,
                shape = androidx.compose.foundation.shape.RoundedCornerShape(50)
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

/**
 * ラウンドの終わり。**1 行目で何枚残ったかを答える。**
 *
 * `upcoming` が null なら、これ以上は進めない。**なぜ進めないかを言う。**
 * 押せないボタンだけ置いても、理由が分からない。
 */
@Composable
private fun RoundDone(
    session: Session,
    upcoming: Session?,
    onNext: (Session) -> Unit,
    onBack: () -> Unit
) {
    val kept = session.survivors.size
    // 見たのは代表の数。**枚数で言うために仲間を足す。**
    val seen = session.history.sumOf { decision ->
        decision.group.sumOf { session.members[it]?.size ?: 1 }
    }
    val keptPhotos = session.survivors.sumOf { session.members[it]?.size ?: 1 }
    Column(
        Modifier.fillMaxSize().padding(24.dp),
        verticalArrangement = Arrangement.Center
    ) {
        Text("ROUND ${session.round} 完了", fontSize = 12.sp, color = Lime)
        Text(
            "★${session.targetStar + 1} が $keptPhotos 枚",
            fontSize = 22.sp, fontWeight = FontWeight.Bold
        )
        Text(
            // **単位を混ぜない。** 「3 組（4 枚）」は、どちらの数を読めばよいか
            // 分からない。枚数で言い切って、まとめたことは後ろに添える。
            if (keptPhotos == kept) "$seen 枚から $keptPhotos 枚に絞られました。"
            else "$seen 枚から $keptPhotos 枚に絞られました（$kept 組にまとめて選びました）。",
            fontSize = 13.sp, color = Faint, modifier = Modifier.padding(top = 6.dp)
        )

        Spacer(Modifier.height(24.dp))

        if (upcoming != null) {
            Button(
                onClick = { onNext(upcoming) },
                shape = androidx.compose.foundation.shape.RoundedCornerShape(50)
            ) {
                Text(
                    "★${upcoming.targetStar + 1} を選ぶ（${upcoming.queue.size + upcoming.current.size} 組）",
                    fontWeight = FontWeight.Bold
                )
            }
            Spacer(Modifier.height(8.dp))
            TextButton(onClick = onBack) { Text("ここで終える") }
        } else {
            Text(
                if (session.targetStar + 1 >= 4) "★5 まで来ました。これ以上は上げられません。"
                else "残りが $kept 枚では、次のラウンドで比べる相手がいません。",
                fontSize = 13.sp, color = Faint
            )
            Spacer(Modifier.height(16.dp))
            Button(
                onClick = onBack,
                shape = androidx.compose.foundation.shape.RoundedCornerShape(50)
            ) {
                Text("アルバムへ戻る", fontWeight = FontWeight.Bold)
            }
        }
    }
}
