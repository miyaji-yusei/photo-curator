@file:OptIn(
    androidx.compose.foundation.ExperimentalFoundationApi::class,
    androidx.compose.material3.ExperimentalMaterial3Api::class
)


package app.photocurator.next

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
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
import uniffi.photo_curator_core.PairOverride
import uniffi.photo_curator_core.nextRound
import uniffi.photo_curator_core.regroup
import uniffi.photo_curator_core.setRepresentative
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
fun CullScreen(project: Project, onResults: () -> Unit, onBack: () -> Unit) {
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
    // 長押しで大きく見ている 1 枚。**選別の判断はここでは動かさない。**
    var zooming by remember { mutableStateOf<Photo?>(null) }
    // 開いている連写のまとまり（代表の相対パス）。
    var editingBurst by remember { mutableStateOf<String?>(null) }
    // 人が手で直したまとめ方。**基準より優先される。**
    var overrides by remember { mutableStateOf<List<PairOverride>>(emptyList()) }
    // 学習した「見た目が近い」の境目。学習していなければ既定値。
    var learnedDistance by remember { mutableStateOf(DEFAULT_DISTANCE) }

    // 相対パスから写真を引く。core は相対パスしか知らない。
    val byPath = remember(photos) { photos.associateBy { it.relativePath } }

    val threshold = remember(learnedDistance) { thresholdFor(learnedDistance) }

    LaunchedEffect(project.id) {
        note = "写真を読み込んでいます…"
        // 指紋は OS の縮小画像から作るので**原本を読まない**（1 枚 3ms）。
        // **前に作った分は作り直さない。**
        note = "似た写真を調べています…"
        val prepared0 = Prepare.run(context, project) { done, total -> prepared = done to total }
        photos = prepared0.first
        refs = prepared0.second
        // **読んだ値はその場の変数で使う。**
        // state に入れてから同じ効果の中で読むと、まだ再構成されていない
        // 古い値を掴む。実際それで、学習した基準も手直しも効いていなかった。
        val distance = Learning.learned(context, project.id) ?: DEFAULT_DISTANCE
        val loadedThreshold = thresholdFor(distance)
        val loadedOverrides = Overrides.load(context, project.id)
        learnedDistance = distance
        overrides = loadedOverrides
        Neighbours.log(refs, loadedThreshold)

        // **途中があれば続きから。** 無ければ新しく始める。
        val saved = Store.load(context, project.id)
        session = saved ?: startRound(
            refs,
            groupSize = Prefs.groupSize(context).toUInt(),
            targetStar = 0,
            groupBursts = Prefs.groupBursts(context),
            threshold = loadedThreshold,
            overrides = loadedOverrides
        )
        note = if (saved != null) "続きから" else ""
    }

    val live = session
    if (live == null) {
        Box(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing)) {
            // **待っている間も出口を残す。** 919 枚のアルバムでは 1 分近くかかる。
            // 戻れない画面に入れてしまうと、待つ以外に何もできなくなる。
            IconButton(onClick = onBack, modifier = Modifier.align(Alignment.TopStart)) {
                Icon(Icons.Filled.ArrowBack, "やめて戻る")
            }
            Column(
                Modifier.align(Alignment.Center),
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
                    Text(
                        "ここで戻っても、調べた分はとってあります",
                        color = Faint, fontSize = 11.sp,
                        modifier = Modifier.padding(top = 6.dp)
                    )
                }
            }
        }
        return
    }

    /** 確定して次へ。**確定のたびに保存する。どこで止めても失わない。** */
    fun commit(picked: Set<String>) {
        val next = advance(live, picked.toList())
        session = next
        selected = emptySet()
        // **複数モードは切らない。** 複数で選ぶ人はずっと複数で選ぶので、
        // 毎回押し直させるのは 1 グループにつき 1 タップ増えるのと同じ。
        scope.launch { Store.save(context, project.id, next) }
    }

    fun stepBack() {
        val next = undo(live)
        session = next
        selected = emptySet()
        scope.launch { Store.save(context, project.id, next) }
    }

    /**
     * まとめ方を直して、**その場で組み直す。**
     *
     * 次のラウンドまで待たせると「押したのに何も起きない」と同じになる。
     * 決めた写真はそのまま、進んだ分は星として残る（core の regroup）。
     */
    fun applyOverrides(added: List<PairOverride>) {
        if (added.isEmpty()) return
        val merged = Overrides.merged(overrides, added)
        overrides = merged
        val next = regroup(live, refs, true, threshold, merged)
        session = next
        selected = emptySet()
        scope.launch {
            Overrides.save(context, project.id, merged)
            Store.save(context, project.id, next)
        }
    }

    /** まとまりの中の隣どうしを全部切る。 */
    fun splitBurst(rep: String) {
        val mates = live.members[rep] ?: return
        applyOverrides(
            mates.zipWithNext().map { (left, right) ->
                PairOverride(left = left, right = right, decision = "split")
            }
        )
    }

    /**
     * いま選んでいる代表どうしを 1 つのまとまりにする。
     *
     * **繋げるのは画面で隣り合っているものだけ。** 代表は撮影順に並んでいるので、
     * 隣り合う代表の境目は「前のまとまりの最後」と「次のまとまりの最初」になる。
     * 飛び石で繋ぐと、あいだの写真がどちらに属するのか説明できなくなる。
     */
    fun joinSelected() {
        val order = live.current.withIndex().filter { it.value in selected }.map { it.index }
        if (order.size < 2) return
        if (order.last() - order.first() != order.size - 1) return
        val reps = order.map { live.current[it] }
        applyOverrides(
            reps.zipWithNext().map { (left, right) ->
                PairOverride(
                    // まとまりの端どうしを繋ぐ。代表は先頭とは限らないので、
                    // 仲間の並び（撮影順）から取る。
                    left = live.members[left]?.last() ?: left,
                    right = live.members[right]?.first() ?: right,
                    decision = "join"
                )
            }
        )
    }

    /** 隣り合う 2 つ以上を選んでいるか。**繋げるときだけボタンを出す。** */
    val canJoin = run {
        val order = live.current.withIndex().filter { it.value in selected }.map { it.index }
        order.size >= 2 && order.last() - order.first() == order.size - 1
    }

    // 連写のまとまりを開いているとき。**選び直しても星は動かない**ので、
    // 保存はするが判断としては何も進めない。
    editingBurst?.let { rep ->
        val mates = live.members[rep]
        if (mates == null) {
            editingBurst = null
        } else {
            BurstSheet(
                members = mates,
                shown = rep,
                byPath = byPath,
                onPick = { wanted ->
                    val next = setRepresentative(live, rep, wanted)
                    if (next != null) {
                        session = next
                        // 選ばれていた印は代表について回る。取り違えないよう外す。
                        selected = emptySet()
                        scope.launch { Store.save(context, project.id, next) }
                    }
                    editingBurst = null
                },
                onZoom = { photo ->
                    editingBurst = null
                    zooming = photo
                },
                onSplit = {
                    editingBurst = null
                    splitBurst(rep)
                },
                onDismiss = { editingBurst = null }
            )
        }
    }

    zooming?.let { photo ->
        ZoomView(photo = photo, onClose = { zooming = null })
        return
    }

    Column(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing)) {
        CullBar(
            project = project,
            session = live,
            selectedCount = selected.size,
            multi = multi,
            onBack = onBack,
            onUndo = { stepBack() },
            onToggleMulti = { multi = !multi; selected = emptySet() },
            onClear = { selected = emptySet() },
            canJoin = canJoin,
            onJoin = { joinSelected() },
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
                nextRound(live, refs, true, threshold, overrides)
            }
            RoundDone(
                session = live,
                upcoming = upcoming,
                onNext = { next ->
                    session = next
                    selected = emptySet()
                    scope.launch { Store.save(context, project.id, next) }
                },
                onResults = onResults,
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
                                    // この 1 枚が何枚ぶんの代表か。
                                    stands = live.members[path]?.size ?: 1,
                                    onHold = { zooming = byPath[path] },
                                    onOpenBurst = { editingBurst = path },
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
    project: Project,
    session: Session,
    selectedCount: Int,
    multi: Boolean,
    onBack: () -> Unit,
    onUndo: () -> Unit,
    onToggleMulti: () -> Unit,
    onClear: () -> Unit,
    canJoin: Boolean,
    onJoin: () -> Unit,
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
            // 隣り合う代表を選んでいるときだけ。**繋げないときは出さない。**
            if (canJoin) {
                TextButton(onClick = onJoin) {
                    Text("ひとまとまりにする", fontSize = 12.sp, color = Lime)
                }
            }
        }

        Column(Modifier.weight(1f), horizontalAlignment = Alignment.CenterHorizontally) {
            if (session.finished) {
                Text(project.name, fontSize = 13.sp)
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
private fun Tile(
    number: Int,
    photo: Photo?,
    picked: Boolean,
    stands: Int,
    onTap: () -> Unit,
    onHold: () -> Unit,
    onOpenBurst: () -> Unit
) {
    Box(
        Modifier
            .fillMaxSize()
            .clip(androidx.compose.foundation.shape.RoundedCornerShape(8.dp))
            .background(Tile)
            .then(if (picked) Modifier.border(3.dp, Lime) else Modifier)
            // 押したら決まる、長押しなら大きく見る。**確定が 1 タップのまま**
            // 残るように、拡大は別の動作に逃がす。
            .combinedClickable(
                enabled = photo != null,
                onClick = onTap,
                onLongClick = onHold
            )
    ) {
        if (photo == null) {
            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                Text("読めません", fontSize = 11.sp, color = Faint)
            }
        } else {
            AsyncImage(
                model = ImageRequest.Builder(LocalContext.current)
                    // **ここは大きく出すので原本を読む。**
                    // EXIF の縮小画像は 160x120 しかなく、選別の判断には足りない。
                    // 一度読めば端末に残るので、2 回目からは網に行かない。
                    .data(photo.fullModel)
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

        // **畳んだことを画面でも言う。** 694 枚が 666 組になった理由が
        // どこにも出ていないと、消えたのではないかと思われる。
        // 残せば仲間にも同じ星が付くので、そのことも読み取れる必要がある。
        if (stands > 1) {
            Box(
                Modifier
                    .align(Alignment.TopEnd)
                    .padding(6.dp)
                    .clip(androidx.compose.foundation.shape.RoundedCornerShape(6.dp))
                    .background(Color(0xB3101114))
                    // **バッジは押せる。** ここを押すと中身が開く。
                    // タイル本体の「押したら確定」を邪魔しないよう、
                    // この当たり判定が先に受け取る。
                    .clickable(onClick = onOpenBurst)
                    .padding(horizontal = 7.dp, vertical = 2.dp)
            ) {
                Text("連写 $stands 枚", fontSize = 11.sp, color = Lime)
            }
        }
    }
}

/**
 * ラウンド完了。**1 行目で何枚残ったかを答える。**
 *
 * 星の内訳を添えるのは、「絞れた」だけでは**どこに溜まったのか**が
 * 分からないため。次のラウンドで何を見るのかも先に言う。
 *
 * `upcoming` が null なら、これ以上は進めない。**なぜ進めないかを言う。**
 * 押せないボタンだけ置いても、理由が分からない。
 */
@Composable
private fun RoundDone(
    session: Session,
    upcoming: Session?,
    onNext: (Session) -> Unit,
    onResults: () -> Unit,
    onBack: () -> Unit
) {
    val kept = session.survivors.size
    // 見たのは代表の数。**枚数で言うために仲間を足す。**
    val seen = session.history.sumOf { decision ->
        decision.group.sumOf { session.members[it]?.size ?: 1 }
    }
    val keptPhotos = session.survivors.sumOf { session.members[it]?.size ?: 1 }
    val counts = IntArray(6)
    for (star in session.ratings.values) counts[star.coerceIn(0, 5)] += 1
    val total = session.ratings.size

    Column(
        Modifier.fillMaxSize().padding(horizontal = 24.dp),
        verticalArrangement = Arrangement.Center
    ) {
        Reading {
            Text("ROUND ${session.round} 完了", fontSize = 12.sp, color = Lime)
            Text(
                "★${session.targetStar + 1} が $keptPhotos 枚残りました",
                fontSize = 22.sp, fontWeight = FontWeight.Bold
            )
            Text(
                // **単位を混ぜない。** 「3 組（4 枚）」は、どちらの数を読めばよいか
                // 分からない。枚数で言い切って、まとめたことは後ろに添える。
                if (keptPhotos == kept) "$seen 枚から $keptPhotos 枚に絞られました。"
                else "$seen 枚から $keptPhotos 枚に絞られました（$kept 組にまとめて選びました）。",
                fontSize = 13.sp, color = Faint, modifier = Modifier.padding(top = 6.dp)
            )

            // ---- 星の内訳。**どこに溜まったかを 1 本の帯で。** ----
            Spacer(Modifier.height(18.dp))
            Row(
                Modifier.fillMaxWidth().height(6.dp)
                    .clip(androidx.compose.foundation.shape.RoundedCornerShape(3.dp))
            ) {
                for (star in 5 downTo 0) {
                    if (counts[star] == 0) continue
                    Box(
                        Modifier
                            .weight(counts[star].toFloat())
                            .fillMaxHeight()
                            .background(if (star > 0) Lime.copy(alpha = 0.35f + star * 0.13f) else Color(0xFF2C3038))
                    )
                }
            }
            Spacer(Modifier.height(8.dp))
            Text(
                (5 downTo 0).filter { counts[it] > 0 }
                    .joinToString(" · ") { "★$it ${counts[it]} 枚" },
                fontSize = 12.sp, color = Faint
            )

            Spacer(Modifier.height(18.dp))

            if (upcoming != null) {
                val nextGroups = upcoming.queue.size + upcoming.current.size
                val turns = (nextGroups + upcoming.groupSize.toInt() - 1) /
                    maxOf(1, upcoming.groupSize.toInt())
                Text(
                    "次は ★${session.targetStar + 1} の $keptPhotos 枚を " +
                        "${upcoming.groupSize} 枚ずつ見比べます（約 $turns 回）。" +
                        "ここで終えても、結果はいつでも開けます。",
                    fontSize = 13.sp, color = Faint
                )
                Spacer(Modifier.height(16.dp))
                Row(verticalAlignment = Alignment.CenterVertically) {
                    TextButton(onClick = onResults) { Text("ここで終えて結果へ") }
                    Spacer(Modifier.weight(1f))
                    Button(
                        onClick = { onNext(upcoming) },
                        shape = androidx.compose.foundation.shape.RoundedCornerShape(50)
                    ) {
                        Text(
                            "★${upcoming.targetStar} を選別する（ROUND ${upcoming.round}）",
                            fontWeight = FontWeight.Bold
                        )
                    }
                }
            } else {
                Text(
                    if (session.targetStar + 1 >= MAX_ROUND_STAR)
                        "★$MAX_ROUND_STAR まで来ました。これ以上は上げられません。"
                    else "残りが $kept 枚では、次のラウンドで比べる相手がいません。",
                    fontSize = 13.sp, color = Faint
                )
                Spacer(Modifier.height(16.dp))
                Row(verticalAlignment = Alignment.CenterVertically) {
                    TextButton(onClick = onBack) { Text("プロジェクトへ戻る") }
                    Spacer(Modifier.weight(1f))
                    Button(
                        onClick = onResults,
                        shape = androidx.compose.foundation.shape.RoundedCornerShape(50)
                    ) {
                        Text("結果を見る", fontWeight = FontWeight.Bold)
                    }
                }
            }
        }
    }
}

/** 星の上限。core の MAX_STAR と揃える。 */
private const val MAX_ROUND_STAR = 5
