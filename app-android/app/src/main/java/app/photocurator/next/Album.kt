@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package app.photocurator.next

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
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
import androidx.compose.runtime.collectAsState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
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
    // 解析できなかった写真の控え（U58）。一覧と、非対応を引いた分母に使う。
    var failures by remember { mutableStateOf<Map<String, Failure>>(emptyMap()) }
    var showFailures by remember { mutableStateOf(false) }
    val failureRows = remember(photos, failures) { Failures.rows(photos, failures) }
    // 選別の対象の枚数（非対応を除く）。準備の分母。
    val workable = remember(photos, failures) { Failures.workableCount(photos, failures) }
    var filter by remember { mutableStateOf("all") }
    var menu by remember { mutableStateOf(false) }
    var confirmRestart by remember { mutableStateOf(false) }
    var reloads by remember { mutableStateOf(0) }
    // 「写真を再読み込み」を押されたか。**押されたときだけ網へ行く。**
    var rescan by remember { mutableStateOf(false) }
    // 開始前の確認を出しているか。**初回は必ず出す。**
    var starting by remember { mutableStateOf(false) }
    // 「…」から開くもの。
    var renaming by remember { mutableStateOf(false) }
    var technical by remember { mutableStateOf(false) }
    var removing by remember { mutableStateOf(false) }
    // 準備でつまずいたこと。（人の言葉, 技術文言）。**詳細は開いたときだけ出す。**
    var trouble by remember { mutableStateOf<Pair<String, String>?>(null) }
    var troubleDetail by remember { mutableStateOf(false) }
    // 大きく見ている並びと、その何枚目か。**ここは見るだけ**なので星は動かない。
    var zooming by remember { mutableStateOf<Pair<List<Photo>, Int>?>(null) }
    // 一覧のスクロール位置。**拡大を開くと下の一覧は組まれなくなる**が、位置はここに残して、
    // 閉じたときに戻す（A10）。早期 return より前に置くこと。
    val gridState = androidx.compose.foundation.lazy.grid.rememberLazyGridState()
    // サイドカーとの食い違い（両方とも進んでいて違う）。5 択で選ばせる。
    var clash by remember { mutableStateOf<SidecarClash?>(null) }
    // 同期の結果を 1 行で。**黙って書かない、黙って失敗しない。**
    var syncNote by remember { mutableStateOf<String?>(null) }
    var syncBad by remember { mutableStateOf(false) }
    // NAS を確かめている間は**選別を始めさせない**（確かめる前に始めると、NAS の版を一度も
    // 見ないまま進み、あとで食い違う。設計書 §2.3・§4.5）。
    var checking by remember { mutableStateOf(Sidecar.supports(project)) }
    // 「この端末の状況を残す」を選んだあと（NAS から切り離し、自動で書かない）。
    var detached by remember { mutableStateOf(Sidecar.detached(context, project)) }

    // U49: 同名の JPEG がある RAW を外すか（プロジェクトごと、既定オン）。
    var pairRaw by remember(project.id) { mutableStateOf(Prefs.pairRawJpeg(context, project.id)) }
    // 切り替えたら「再読み込みで反映」を出す。**再読み込みしたら消す。**
    var pairRawChanged by remember(project.id) { mutableStateOf(false) }

    /** 同期の結果を画面に映す。取り込んだら読み直す。 */
    suspend fun show(outcome: SyncOutcome) {
        when (outcome) {
            is SyncOutcome.Asking -> clash = outcome.clash
            is SyncOutcome.Pulled -> session = Store.load(context, project.id)
            else -> Unit
        }
        // U51: ほかの端末の設定を取り込んだ。スイッチを合わせ、再読み込みを促す（自動では取り直さない）。
        outcome.settingsAdopted?.let {
            pairRaw = it
            pairRawChanged = true
        }
        Sidecar.note(outcome)?.let {
            syncNote = it
            syncBad = outcome is SyncOutcome.Blocked
        }
        detached = Sidecar.detached(context, project)
    }

    /**
     * 選別を始める（続ける）前に、もう一度 NAS と合わせる（設計書 §4.5）。
     * 相手が着手済みでこの端末が未着手なら、ここで取り込んでから始める。確認が要るなら始めない。
     */
    suspend fun syncBeforeCull(): Boolean {
        if (!Sidecar.supports(project)) return true
        checking = true
        val outcome = try {
            Sidecar.check(context, project)
        } finally {
            checking = false
        }
        show(outcome)
        return outcome !is SyncOutcome.Asking
    }

    // **開いたら準備が動き出す。** カードに数が出ているのに何も進まないと、
    // 止まっているのか終わっているのか分からない。
    var preparing by remember { mutableStateOf(0 to 0) }
    // 表示用画像の進み。**NAS のときだけ動く。**
    var rendering by remember { mutableStateOf(0 to 0) }
    var displayEdge by remember { mutableStateOf(Prefs.projectEdge(context, project.id)) }
    // 「…」から大きさを選んでいるか。
    var choosingEdge by remember { mutableStateOf(false) }
    // 一覧の列数。**0 は「おまかせ」**（幅から決める）。
    var columns by remember { mutableStateOf(Prefs.gridColumns(context)) }

    // 準備の進みを見る。**持ち主はアプリなので、画面はただ映すだけ。**
    val prepared by Preparations.watch().collectAsState()
    // **このプロジェクトの進みだけを鍵にする**（A12）。全体の Map を鍵にすると、他のプロジェクトの
    // 準備が 10 枚進むたびに、止まっているこちらの控え（一覧・ハッシュ値）を全部読み直していた。
    LaunchedEffect(prepared[project.id], project.id) {
        val mine = prepared[project.id] ?: return@LaunchedEffect
        preparing = mine.meta
        rendering = mine.display
        if (mine.trouble != null) trouble = mine.trouble to mine.trouble
        // 進んでいるあいだも、できた分を読み直して一覧に出す。
        if (!mine.running) {
            photos = Listing.load(context, project.source.key) ?: photos
            prints = Fingerprints.load(context, project.source.key)
            failures = Failures.load(context, project.source.key)
        }
    }

    LaunchedEffect(project.id, reloads) {
        scanned = false
        trouble = null
        session = Store.load(context, project.id)
        prints = Fingerprints.load(context, project.source.key)
        failures = Failures.load(context, project.source.key)
        // **転んだままなら、まずそれを出す。** 準備をやり直すのは押されたとき。
        val noted = Trouble.load(context, project.source.key)
        photos = Listing.load(context, project.source.key)
            ?: if (noted != null) emptyList()
            else Photos.forSource(context, project.source, Prefs.pairRawJpeg(context, project.id))
        scanned = true
        try {
            // 準備は**アプリが持つ**（Preparations）。画面を離れても止まらないので、
            // 別のプロジェクトを選別しているあいだにも進む。
            Preparations.ensure(context, project, rescan, displayEdge) {
                // 終わったら顔ぶれとハッシュ値を読み直す。
                scope.launch {
                    photos = Listing.load(context, project.source.key) ?: photos
                    prints = Fingerprints.load(context, project.source.key)
                    failures = Failures.load(context, project.source.key)
                    // 出せない大きさだったら準備の側で直している（設計 08 章 8.5）。
                    displayEdge = Prefs.projectEdge(context, project.id)
                }
            }
            rescan = false

            // ---- サイドカー ----
            // 開いたとき・戻ったときに確かめる。判断は core（意味が同じなら何もしない／
            // 片方が未着手なら確認なしに合わせる／両方着手で違えば 5 択）。
            // **選別画面を離れたときの書き込みと同じ列に並ぶ**ので、それを追い越さない。
            if (Sidecar.supports(project)) {
                checking = true
                val outcome = try {
                    Sidecar.check(context, project)
                } finally {
                    checking = false
                }
                show(outcome)
            }

            // ---- Amazon のリンク ----
            // **開いたときに 1 回だけ確かめる。** 消えていても、準備済みの絵で選別は
            // 続けられる。できなくなることだけを言う（設計 08 章 8.4）。
            if (project.source.kind == SourceKind.Amazon) {
                val alive = Amazon.share(Amazon.linkOf(project.source.key))
                if (alive is SmbResult.Failed) {
                    syncNote = if (alive.reason == Amazon.GONE)
                        "Amazon のリンクが削除されています。準備済みの画像で選別は続けられますが、" +
                            "拡大とギャラリーへの保存はできません"
                    else alive.reason
                }
            }
        } catch (error: kotlinx.coroutines.CancellationException) {
            throw error
        } catch (error: Exception) {
            // **黙って落とさない。** 何が起きたかを 1 文にして、ホームにも残す。
            val said = Smb.describe(error)
            trouble = said to (error.message ?: error.javaClass.name)
            Trouble.note(context, project.source.key, said)
            rescan = false
            // NAS を確かめる前に転んだ。**始められないままにしない**（選別は端末だけでもできる）。
            checking = false
        }
    }

    // **閉じるときにも書く。** 開いたときだけだと、詳細で星を直して
    // そのまま閉じた分が渡らない。
    DisposableEffect(project.id) {
        onDispose { Sidecar.pushIfChanged(context, project) }
    }

    val live = session
    val ratings = live?.ratings ?: emptyMap()
    // 準備の進み（10 枚ごと）で組み直されても、全写真を数え直さない（A12）。
    val starred = remember(photos, ratings) { photos.count { (ratings[it.relativePath] ?: 0) > 0 } }
    val photoSpan = remember(photos) { span(photos) }
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
            // **名前は残りの幅で省略する。** 閉じた幅だと名前がボタンを押しつぶし、文字が縦に折れた。
            Text(
                project.name, fontSize = 18.sp, fontWeight = FontWeight.SemiBold,
                maxLines = 1, overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f)
            )
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
                        else -> scope.launch { if (syncBeforeCull()) onCull(false) }
                    }
                },
                // **走査が終わるまで始められない。** 枚数と時間順が決まらないため。
                // **NAS を確かめ終えるまでも始められない**（つながらなければ、そこで押せるようになる）。
                enabled = scanned && photos.isNotEmpty() && !checking,
                shape = RoundedCornerShape(50)
            ) {
                Icon(Icons.Filled.PlayArrow, null, Modifier.size(18.dp))
                Spacer(Modifier.width(6.dp))
                Text(
                    when {
                        !scanned -> "走査中…"
                        checking -> "NAS を確認中…"
                        live == null -> "選別を開始"
                        live.finished -> "結果を見る"
                        else -> "選別を続ける"
                    },
                    fontWeight = FontWeight.Bold, fontSize = 14.sp,
                    maxLines = 1, softWrap = false
                )
            }
        }

        // ---- 左: 出所 / 準備 / 選別 ----
        val cards: @Composable () -> Unit = {
                Card {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Icon(
                            sourceIcon(project.source.kind),
                            null, Modifier.size(16.dp), tint = Faint
                        )
                        Spacer(Modifier.width(8.dp))
                        Text(project.source.label, fontSize = 13.sp)
                    }
                    Text(
                        // **いつ撮ったものかを 1 行で。** 同じ名前のフォルダが
                        // 並んだとき、枚数だけでは見分けがつかない。
                        "$workable 枚" + photoSpan?.let { " · $it" }.orEmpty(),
                        fontSize = 12.sp, color = Faint,
                        modifier = Modifier.padding(top = 6.dp)
                    )
                    syncNote?.let { note ->
                        Text(
                            note,
                            fontSize = 11.sp,
                            color = if (syncBad || note.contains("できません")) Warn else Sky,
                            modifier = Modifier.padding(top = 6.dp)
                        )
                    }
                    // 「この端末の状況を残す」を選んだあと。**NAS とは別の結果であることを出し続ける。**
                    if (detached) {
                        Row(
                            Modifier.padding(top = 6.dp),
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            Text(
                                "この端末だけの結果（NAS とは別）",
                                fontSize = 11.sp, color = Warn,
                                modifier = Modifier.weight(1f)
                            )
                            TextButton(onClick = {
                                syncNote = "この端末の状況を NAS に書いています…"
                                syncBad = false
                                scope.launch { show(Sidecar.writeNow(context, project)) }
                            }) { Text("NAS に書き込む", fontSize = 12.sp) }
                        }
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
                                scanned && prints.size >= workable &&
                                    (!project.source.remote ||
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
                            when (project.source.kind) {
                                SourceKind.Nas -> "同じ Wi-Fi につながっているか、NAS の電源を確かめてください"
                                SourceKind.Amazon -> "ネットワークと、Amazon Photos のリンクが残っているかを確かめてください"
                                SourceKind.Album -> "写真へのアクセスが許可されているか確かめてください"
                            },
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
                    // **非対応は分母に入れない**（U58）。以降の 2 行は選別の対象の枚数で数える。
                    // Tauri 版の「表示用画像」に当たる段。ネイティブでは OS の縮小画像を
                    // そのまま使うので、実際に作るのは連写のまとめに使うハッシュ値だけ。
                    // ハッシュ値は撮影時刻と同じ 1 回の読みで取れるので、同じ行に畳む。
                    PrepRow(
                        "撮影時刻・サムネイル",
                        maxOf(prints.size, preparing.first),
                        workable,
                        prints.size >= workable && photos.isNotEmpty()
                    )
                    // **網越しのときだけ。** 端末の写真は手元でデコードすれば足りる。
                    if (project.source.remote) {
                        PrepRow(
                            "表示用画像（${displayEdge}px）",
                            rendering.first,
                            workable,
                            rendering.second > 0 && rendering.first >= rendering.second
                        )
                    }
                    // 解析できなかったファイル。**押すと名前と理由の一覧。**
                    if (failureRows.isNotEmpty()) {
                        val unsupportedRows = failureRows.count { it.kind == FailureKind.Unsupported }
                        Text(
                            "${failureRows.size} 件を解析できませんでした" +
                                if (unsupportedRows > 0) "（うち ${unsupportedRows} 件は対応していない形式）" else "",
                            fontSize = 12.sp, color = Warn,
                            modifier = Modifier
                                .fillMaxWidth()
                                .clip(RoundedCornerShape(8.dp))
                                .clickable { showFailures = true }
                                .padding(top = 8.dp, bottom = 4.dp)
                        )
                    }
                    Text(
                        when (project.source.kind) {
                            SourceKind.Nas -> "できた写真から選別に出ます。NAS から読むので Wi-Fi 推奨"
                            SourceKind.Amazon -> "できた写真から選別に出ます。Amazon から縮小した絵を受け取るので Wi-Fi 推奨"
                            SourceKind.Album -> "できた写真から選別に出ます"
                        },
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
                            "★1 以上 $starred 枚 · ${if (live.groupSize.toInt() == Prefs.SLIDESHOW_SIZE) "スライドショー" else "${live.groupSize} 枚ずつ"} · 連写まとめ on",
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
        // おまかせが何列になるかを知るために、置かれた幅を測る。
        var listWidth by remember { mutableStateOf(0) }
        val gallery: @Composable () -> Unit = {
                // 幅からの既定を先に知る。**同じ数のボタンを 2 つ置かない**
                // （「おまかせ」と「5 列」が同じ意味になってしまう）。
                val auto = maxOf(2, (listWidth / 96).coerceAtMost(8))
                Row(
                    Modifier
                        .fillMaxWidth()
                        .padding(bottom = 8.dp)
                        // **狭い画面では横に送る。** 入りきらないとチップが
                        // 1 文字ずつ折り返されて縦長の帯になる（カバー画面で発生）。
                        .horizontalScroll(rememberScrollState()),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Chip("すべて ${photos.size}", filter == "all") { filter = "all" }
                    if (starred > 0) Chip("★1 以上 $starred", filter == "star") { filter = "star" }
                    if (bursts > 0) Chip("連写 $bursts 組", filter == "burst") { filter = "burst" }
                    Spacer(Modifier.width(16.dp))
                    // 列数。**おまかせ＋その幅では選べない数**だけを出す。
                    Chip("おまかせ", columns == 0) {
                        columns = 0; Prefs.setGridColumns(context, 0)
                    }
                    for (n in 2..5) {
                        if (n == auto) continue
                        Chip("$n 列", columns == n) {
                            columns = n; Prefs.setGridColumns(context, n)
                        }
                    }
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
                    state = gridState,
                    columns = if (columns > 0) GridCells.Fixed(columns)
                    else GridCells.Adaptive(minSize = 96.dp),
                    verticalArrangement = Arrangement.spacedBy(4.dp),
                    horizontalArrangement = Arrangement.spacedBy(4.dp)
                ) {
                    items(shown, key = { it.relativePath }) { photo ->
                        // **なぜ絵が無いのかを、タイルの中で言う。**
                        // 読めない形式・まだ作っていない・作れなかった、を分ける。
                        val format = remember(photo.relativePath) { unsupportedFormat(photo) }
                        var state by remember(photo.relativePath) {
                            mutableStateOf(
                                when {
                                    format != null -> Preview.Unsupported
                                    // ハッシュ値が出来ていれば、その 1 回の読みで
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
                            EmptyTile(state, format, raw = photo.isRaw)
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
                    Column(
                        Modifier.weight(1f).onSizeChanged { listWidth = (it.width / 2.625f).toInt() }
                    ) { gallery() }
                }
            } else {
                Row(Modifier.fillMaxSize()) {
                    Column(Modifier.width(340.dp).verticalScrollable()) { cards() }
                    Spacer(Modifier.width(16.dp))
                    Column(
                        Modifier.weight(1f).onSizeChanged { listWidth = (it.width / 2.625f).toInt() }
                    ) { gallery() }
                }
            }
        }
    }

    if (starting) {
        StartSheet(
            project = project,
            photoCount = workable,
            readyCount = if (project.source.remote) rendering.first else workable,
            onStart = {
                starting = false
                // 連写をまとめる設定で、まだ基準を決めていなければ学習へ。
                // **一度決めたら二度は聞かない。**
                scope.launch {
                    // **始める前に NAS と合わせる。** 相手の続きがあればここで取り込む。
                    if (!syncBeforeCull()) return@launch
                    val needsLearning = Prefs.groupBursts(context) &&
                        Learning.learned(context, project.id) == null
                    onCull(needsLearning)
                }
            },
            onDismiss = { starting = false }
        )
    }

    if (menu) {
        ModalBottomSheet(
        onDismissRequest = { menu = false },
        containerColor = Surface,
        // **下の帯まで自分の色で塗る。** 既定だとナビゲーションバーの
        // ところが白く残り、一番下のボタンに被る。
        contentWindowInsets = { WindowInsets(0) }
    ) {
            Column(
                Modifier.verticalScroll(rememberScrollState())
                    .padding(horizontal = 8.dp).padding(bottom = 24.dp)
            ) {
                DetailMenuRow("写真を再読み込み") {
                    menu = false; rescan = true; reloads += 1; pairRawChanged = false
                }
                // U49: 組の RAW を外すか。**自動では取り直さない**（PC と同じ。押されたときだけ網へ行く）。
                if (project.source.kind != SourceKind.Amazon) {
                    PairRawSwitch(
                        checked = pairRaw,
                        onChange = { on ->
                            pairRaw = on
                            Prefs.setPairRawJpeg(context, project.id, on)
                            pairRawChanged = true
                            // U51: NAS のプロジェクトはサイドカーにも書く（いつもの区切りの書き込みと同じ列。
                            // 選別状況が同じでも、切り替えた設定が NAS より新しければ書く）。NAS 以外は何もしない。
                            Sidecar.pushIfChanged(context, project)
                        },
                        note = if (pairRawChanged) "「写真を再読み込み」で反映します" else null,
                        modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp)
                    )
                }
                if (project.source.remote) {
                    DetailMenuRow("表示用画像の大きさ（${displayEdge}px）") {
                        menu = false; choosingEdge = true
                    }
                }
                if (Sidecar.supports(project)) {
                    DetailMenuRow("NAS に保存（この端末の結果を書く）") {
                        menu = false
                        syncNote = "NAS に保存しています…"
                        syncBad = false
                        scope.launch {
                            // **開いたときと同じ判断で書く**（NAS の中身を確かめずに上書きしない）。
                            val outcome = Sidecar.save(context, project)
                            show(outcome)
                            if (outcome is SyncOutcome.Settled && outcome.settingsPushed && outcome.settingsAdopted == null) {
                                syncNote = "プロジェクトの設定を NAS に書き込みました"
                            } else if (outcome is SyncOutcome.Settled && outcome.note == null && outcome.settingsAdopted == null) {
                                syncNote = "書く必要はありません（NAS の記録と同じか、まだ選別していません）"
                            }
                        }
                    }
                }
                DetailMenuRow("名前を変更") { menu = false; renaming = true }
                DetailMenuRow("技術情報") { menu = false; technical = true }
                DetailMenuRow("選別を最初からやり直す", danger = true) {
                    menu = false; confirmRestart = true
                }
                DetailMenuRow("このプロジェクトを削除", danger = true) {
                    menu = false; removing = true
                }
            }
        }
    }

    if (choosingEdge) {
        ModalBottomSheet(
        onDismissRequest = { choosingEdge = false },
        containerColor = Surface,
        // **下の帯まで自分の色で塗る。** 既定だとナビゲーションバーの
        // ところが白く残り、一番下のボタンに被る。
        contentWindowInsets = { WindowInsets(0) }
    ) {
            Column(Modifier.padding(horizontal = 20.dp).padding(bottom = 24.dp)) {
                Text("表示用画像の大きさ", fontSize = 16.sp, fontWeight = FontWeight.SemiBold)
                Text(
                    // **作り直しになることを先に言う。** 押してから 20 分待たせない。
                    "このプロジェクトだけに効きます。まだ作っていない大きさは、" +
                        "選んだあとに作り直します",
                    fontSize = 12.sp, color = Faint,
                    modifier = Modifier.padding(top = 4.dp, bottom = 12.dp)
                )
                // **出せない大きさは選べない**（Amazon が小さくしか返さない等。設計 08 章 8.5）。
                val maxEdge = Prefs.maxEdgeFor(context, project.source)
                if (Prefs.EDGES.any { !Prefs.edgeAllowed(it, maxEdge) }) {
                    Text(
                        "この共有の写真は長辺 ${maxEdge}px までです",
                        fontSize = 12.sp, color = Faint,
                        modifier = Modifier.padding(bottom = 8.dp)
                    )
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    for (edge in Prefs.EDGES) {
                        Chip("${edge}px", edge == displayEdge, enabled = Prefs.edgeAllowed(edge, maxEdge)) {
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

    // ---- サイドカーの食い違い（両方とも進んでいて違う） ----
    // **プロジェクト単位で選ばせる。写真 1 枚ずつは選ばせない。** 5 択（設計書 §4.6）。
    // どれを選んでも元の 2 つは消さない（NAS の catalog.<端末>.json と、この端末の aside/）。
    clash?.let { asked ->
        SidecarClashDialog(
            clash = asked,
            deviceName = Device.name(),
            onChoose = { choice ->
                clash = null
                syncNote = "NAS の記録と合わせています…"
                syncBad = false
                scope.launch {
                    show(Sidecar.resolve(context, project, asked, choice))
                    // 取り込んだ・混ぜたときは、星と選別の進みを読み直す。
                    session = Store.load(context, project.id)
                }
            },
            onDismiss = {
                // 閉じただけ。**何も変えない。** 選別を始める前にもう一度聞く。
                clash = null
                syncNote = "NAS の記録との食い違いは、まだ選んでいません（始める前にもう一度聞きます）"
                syncBad = true
            }
        )
    }

    if (showFailures) {
        FailuresDialog(failureRows) { showFailures = false }
    }

    if (renaming) {
        var text by remember { mutableStateOf(project.name) }
        AlertDialog(
            onDismissRequest = { renaming = false },
            title = { Text("名前を変更") },
            text = {
                OutlinedTextField(
                    value = text, onValueChange = { text = it },
                    singleLine = true, label = { Text("プロジェクト名") },
                    // **全部消して打ち直せるように**（設計 02「入力欄の決まり」）。
                    trailingIcon = { ClearIcon(text) { text = "" } }
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    val wanted = text.trim()
                    renaming = false
                    // **空にはしない。** 名前が無いカードは探せない。
                    if (wanted.isNotEmpty()) scope.launch {
                        Projects.rename(context, project.id, wanted)
                        reloads += 1
                    }
                }) { Text("変える") }
            },
            dismissButton = { TextButton(onClick = { renaming = false }) { Text("やめる") } }
        )
    }

    if (technical) {
        AlertDialog(
            onDismissRequest = { technical = false },
            title = { Text("技術情報") },
            // **生の指し先はここだけ。** 普段の画面は人の言葉で通す。
            text = {
                Text(
                    """
                        ID: ${project.id}
                        出所: ${project.source.technical}
                        写真: ${photos.size} 枚（非対応 ${photos.size - workable} 枚）/ ハッシュ値 ${prints.size} 枚
                        表示用画像: ${rendering.first} / ${rendering.second}（${displayEdge}px）
                    """.trimIndent(),
                    fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace,
                    fontSize = 12.sp
                )
            },
            confirmButton = { TextButton(onClick = { technical = false }) { Text("閉じる") } }
        )
    }

    if (removing) {
        // **絵は出所ごと。** 同じフォルダを使う別のプロジェクトが残っているなら
        // 消してはいけないし、消すとも言ってはいけない（実際に消していないのに
        // 「14MB 消します」と書いていた）。
        var alone by remember(project.id) { mutableStateOf(false) }
        var bytes by remember(project.id) { mutableStateOf(0L) }
        LaunchedEffect(project.id, removing) {
            alone = ProjectData.othersUsing(context, project) == 0
            val cache = project.source.cacheId
            bytes = if (alone && cache != null) Renders.bytes(context, cache) else 0L
        }
        ConfirmDialog(
            title = "「${project.name}」を削除しますか",
            body = "星・連写のまとめ方・進捗が消えます。写真そのものは変更しません。" +
                (if (bytes > 0) "\n端末の表示用画像 ${bytes / 1024 / 1024}MB も消します。" else "") +
                (if (!alone) "\n同じフォルダの別プロジェクトがあるため表示用画像は残します。" else "") +
                (if (Sidecar.supports(project)) "\nNAS の記録（catalog.json）は消しません。" else ""),
            confirmLabel = "削除",
            onConfirm = {
                removing = false
                scope.launch {
                    ProjectData.remove(context, project)
                    onBack()
                }
            },
            onDismiss = { removing = false }
        )
    }

    if (confirmRestart) {
        ConfirmDialog(
            title = "選別を最初からやり直しますか",
            // **何が消えるかを具体的に言う。**「よろしいですか」では判断できない。
            body = "星と進捗が消えます。連写のまとめ方もリセットされ、次回また質問します。" +
                "写真そのものは変更しません。",
            confirmLabel = "やり直す",
            // 原本には触らないので赤字は出さない。**赤を安売りしない。**
            onConfirm = {
                confirmRestart = false
                scope.launch {
                    ProjectData.restart(context, project.id)
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
private fun Chip(label: String, selected: Boolean, enabled: Boolean = true, onClick: () -> Unit) {
    FilterChip(
        selected = selected,
        onClick = onClick,
        enabled = enabled,
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
