package app.photocurator.next

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import uniffi.photo_curator_core.BurstThreshold
import uniffi.photo_curator_core.PhotoRef
import uniffi.photo_curator_core.advance
import uniffi.photo_curator_core.startRound
import uniffi.photo_curator_core.undo

/**
 * 連写の中身を選別する。**まとまりで 1 つに畳んだ中を、あとから開き直す場所。**
 *
 * 選別では代表しか見えない。代表が通ると仲間も同じ星をもらうので、
 * 「この組の中では 3 枚目が一番いい」という違いは残らない。ここはその
 * 差をつけ直すためだけの画面で、**部品は選別画面と同じ**（同じ並べ方、
 * 同じタイル、同じ「複数」）。覚え直すことを増やさない。
 *
 * **決めるまで星は動かない。** 結果を文で出してから確定させる。
 */
@Composable
fun BurstReviewScreen(
    /** まとまりの中身（撮影順）。代表を含む。 */
    photos: List<Photo>,
    /** このまとまりが今持っている星。ここを土台に上げ下げする。 */
    baseStar: Int,
    displayEdge: Int,
    /** 相対パス → 新しい星。押されたときだけ呼ぶ。 */
    onApply: (Map<String, Int>) -> Unit,
    onZoom: (Int) -> Unit,
    onBack: () -> Unit
) {
    // 連写の中は「似ているもの同士」なので、**まとめ直さない**。
    // 1 枚ずつ並べて見比べる場所であって、畳む場所ではない。
    val refs = remember(photos) {
        photos.map { PhotoRef(it.relativePath, it.takenAt.takeIf { at -> at > 0 }, null, 0) }
    }
    val size = remember(photos) { photos.size.coerceIn(2, 10) }
    var session by remember(photos) {
        mutableStateOf(
            startRound(
                refs,
                groupSize = size.toUInt(),
                targetStar = 0,
                groupBursts = false,
                threshold = BurstThreshold(
                    windowMs = 0, distance = 0u, dHashVersion = Analyse.VERSION
                ),
                overrides = emptyList()
            )
        )
    }
    var selected by remember { mutableStateOf<Set<String>>(emptySet()) }
    var multi by remember { mutableStateOf(false) }
    var stageSize by remember { mutableStateOf(0 to 0) }

    val byPath = remember(photos) { photos.associateBy { it.relativePath } }
    val live = session

    fun commit(picked: Set<String>) {
        session = advance(live, picked.toList())
        selected = emptySet()
    }

    Column(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing)) {
        Row(
            Modifier.fillMaxWidth().height(64.dp).padding(end = 16.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            IconButton(onClick = onBack) { Icon(Icons.Filled.ArrowBack, "戻る") }
            if (live.history.isNotEmpty() && !live.finished) {
                TextButton(onClick = { session = undo(live); selected = emptySet() }) {
                    Text("1 つ戻す", fontSize = 12.sp)
                }
            }
            Column(Modifier.weight(1f)) {
                Text("連写の中身を選別", fontSize = 13.sp, color = Lime)
                Text(
                    "${photos.size} 枚 · いまは全部 ★$baseStar",
                    fontSize = 12.sp, color = Faint
                )
            }
            if (!live.finished) {
                OutlinedButton(
                    onClick = { multi = !multi; selected = emptySet() },
                    shape = RoundedCornerShape(50)
                ) { Text(if (multi) "単数" else "複数", fontSize = 12.sp) }
                Spacer(Modifier.width(10.dp))
                Button(
                    onClick = { commit(selected) },
                    shape = RoundedCornerShape(50)
                ) {
                    Text(
                        if (selected.isEmpty()) "${live.current.size} 枚とも据え置き"
                        else "選んだ ${selected.size} 枚を上げる",
                        fontWeight = FontWeight.Bold, fontSize = 13.sp
                    )
                }
            }
        }

        if (live.finished) {
            // **結果を先に文で出す。** 押す前に何が起きるか分かるように。
            val next = photos.associate {
                it.relativePath to
                    (baseStar + (live.ratings[it.relativePath] ?: 0)).coerceIn(0, 5)
            }
            val risen = next.count { (_, star) -> star > baseStar }
            Column(
                Modifier.fillMaxSize().padding(24.dp),
                verticalArrangement = Arrangement.Center,
                horizontalAlignment = Alignment.CenterHorizontally
            ) {
                Text("この組の中で $risen 枚が上がります", fontSize = 20.sp, fontWeight = FontWeight.Bold)
                Text(
                    if (risen == 0) "全部 ★$baseStar のままです"
                    else "上がった写真は ★${(baseStar + 1).coerceAtMost(5)}、" +
                        "残りは ★$baseStar のままです",
                    fontSize = 13.sp, color = Faint,
                    modifier = Modifier.padding(top = 8.dp)
                )
                Row(Modifier.padding(top = 24.dp), verticalAlignment = Alignment.CenterVertically) {
                    TextButton(onClick = onBack) { Text("やめる") }
                    Spacer(Modifier.width(12.dp))
                    Button(
                        onClick = { onApply(next) },
                        shape = RoundedCornerShape(50)
                    ) { Text("この結果にする", fontWeight = FontWeight.Bold) }
                }
            }
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
                                    // ここは畳まないので、1 枚は必ず 1 枚。
                                    stands = 1,
                                    displayEdge = displayEdge,
                                    onZoom = {
                                        onZoom(photos.indexOfFirst { it.relativePath == path })
                                    },
                                    // ここは ★5 を出さない。**この画面は組の中の上下だけ。**
                                    onHold = {
                                        onZoom(photos.indexOfFirst { it.relativePath == path })
                                    },
                                    onOpenBurst = {},
                                    onTap = {
                                        if (multi) {
                                            selected = if (path in selected) selected - path
                                            else selected + path
                                        } else {
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

        Text(
            "選んだ写真だけ ★${(baseStar + 1).coerceAtMost(5)} に上がります。" +
                "選ばなければ ★$baseStar のまま",
            fontSize = 11.sp, color = Faint,
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp)
        )
    }
}
