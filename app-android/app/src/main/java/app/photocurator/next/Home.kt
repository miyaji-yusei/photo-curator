@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package app.photocurator.next

import android.content.Context
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Dns
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Smartphone
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Settings
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
import androidx.compose.ui.layout.ContentScale
import coil.compose.AsyncImage
import coil.request.ImageRequest
import kotlinx.coroutines.launch

/** カードに出す状態。**状態 1 行・進捗・次の一手**の 3 つに畳む。 */
data class Standing(
    val line: String,
    val detail: String,
    val progress: Float?,
    val tint: Color,
    val action: String,
    /** 完了カードだけ。★0..★5 の枚数。**棒 1 本で結果の形を見せる。** */
    val stars: List<Int>? = null
)

/**
 * ホーム。**プロジェクトを選ぶ。2 回目以降の人に最短で。**
 *
 * キャッチコピーは置かない。1 行目は件数と並び順、右に約束文。
 */
@Composable
fun HomeScreen(
    onOpen: (Project) -> Unit,
    onCreate: () -> Unit,
    onSettings: () -> Unit
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var projects by remember { mutableStateOf<List<Project>>(emptyList()) }
    var standings by remember { mutableStateOf<Map<String, Standing>>(emptyMap()) }
    // カードに出す見本の 1 枚。**端末にあるものだけ**なので、無ければ出さない。
    var covers by remember { mutableStateOf<Map<String, Any>>(emptyMap()) }
    var loaded by remember { mutableStateOf(false) }
    var menuFor by remember { mutableStateOf<Project?>(null) }
    var renaming by remember { mutableStateOf<Project?>(null) }
    var removing by remember { mutableStateOf<Project?>(null) }
    var technical by remember { mutableStateOf<Project?>(null) }
    var reloads by remember { mutableStateOf(0) }

    LaunchedEffect(reloads) {
        projects = Projects.all(context)
        loaded = true
        // 状態は一覧を出してから足す。**印のために一覧を待たせない。**
        standings = projects.associate { it.id to standingOf(context, it) }
        // 見本も同じく後から。**網へは行かない**ので、出なければ出ないまま。
        covers = projects.mapNotNull { project ->
            Covers.forProject(context, project)?.let { project.id to it }
        }.toMap()
    }

    Column(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing)) {
        Row(
            Modifier.fillMaxWidth().height(64.dp).padding(horizontal = 16.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Text("Photo Curator", fontSize = 20.sp, fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.weight(1f))
            IconButton(onClick = onSettings) { Icon(Icons.Filled.Settings, "設定") }
            Spacer(Modifier.width(8.dp))
            Button(onClick = onCreate, shape = RoundedCornerShape(50)) {
                Icon(Icons.Filled.Add, null, Modifier.size(18.dp))
                Spacer(Modifier.width(6.dp))
                Text("プロジェクトを作成", fontWeight = FontWeight.Bold, fontSize = 14.sp)
            }
        }

        Row(
            Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 6.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Text(
                if (projects.isEmpty()) "" else "プロジェクト ${projects.size} 件 · 更新順",
                fontSize = 12.sp, color = Faint
            )
            Spacer(Modifier.weight(1f))
            // 約束文。**毎回同じ場所に置く。**
            Text(
                "写真の原本は読むだけで、移動・削除・書き換えはしません",
                fontSize = 12.sp, color = Faint
            )
        }

        if (!loaded) {
            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                Text("読み込み中…", color = Faint, fontSize = 13.sp)
            }
        } else if (projects.isEmpty()) {
            Column(
                Modifier.fillMaxSize().padding(32.dp),
                verticalArrangement = Arrangement.Center
            ) {
                Text("まだプロジェクトがありません", fontSize = 16.sp, fontWeight = FontWeight.SemiBold)
                Text(
                    "写真のフォルダを選ぶと、原本には触らずに選別を始められます",
                    fontSize = 13.sp, color = Faint, modifier = Modifier.padding(top = 6.dp)
                )
            }
        } else {
            LazyVerticalGrid(
                columns = GridCells.Adaptive(minSize = 320.dp),
                contentPadding = PaddingValues(16.dp),
                horizontalArrangement = Arrangement.spacedBy(12.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp)
            ) {
                items(projects, key = { it.id }) { project ->
                    ProjectCard(
                        project = project,
                        standing = standings[project.id],
                        cover = covers[project.id],
                        onOpen = { onOpen(project) },
                        onMenu = { menuFor = project }
                    )
                }
            }
        }
    }

    menuFor?.let { project ->
        ModalBottomSheet(onDismissRequest = { menuFor = null }, containerColor = Surface) {
            Column(Modifier.padding(horizontal = 8.dp).padding(bottom = 24.dp)) {
                Text(
                    project.name,
                    fontSize = 15.sp, fontWeight = FontWeight.SemiBold,
                    modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp)
                )
                MenuRow("名前を変更") { menuFor = null; renaming = project }
                MenuRow("技術情報") { menuFor = null; technical = project }
                MenuRow("削除", danger = true) { menuFor = null; removing = project }
            }
        }
    }

    renaming?.let { project ->
        var text by remember(project.id) { mutableStateOf(project.name) }
        AlertDialog(
            onDismissRequest = { renaming = null },
            title = { Text("名前を変更") },
            text = {
                OutlinedTextField(
                    value = text, onValueChange = { text = it },
                    singleLine = true, label = { Text("プロジェクト名") }
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    val wanted = text.trim()
                    renaming = null
                    if (wanted.isNotEmpty()) scope.launch {
                        Projects.rename(context, project.id, wanted)
                        reloads += 1
                    }
                }) { Text("変える") }
            },
            dismissButton = { TextButton(onClick = { renaming = null }) { Text("やめる") } }
        )
    }

    technical?.let { project ->
        AlertDialog(
            onDismissRequest = { technical = null },
            title = { Text("技術情報") },
            // **生の指し先はここだけ。** 普段の画面は人の言葉で通す。
            text = { Text("ID: ${project.id}\n出所: ${project.source.technical}") },
            confirmButton = { TextButton(onClick = { technical = null }) { Text("閉じる") } }
        )
    }

    removing?.let { project ->
        AlertDialog(
            onDismissRequest = { removing = null },
            title = { Text("「${project.name}」を削除しますか") },
            // **何が消えて何が消えないかを具体的に言う。**
            text = {
                Text(
                    "このプロジェクトで付けた星と、どこまで見たかが消えます。" +
                        "写真そのものには手を触れません。"
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    removing = null
                    scope.launch { Projects.remove(context, project.id); reloads += 1 }
                }) { Text("削除") }
            },
            dismissButton = { TextButton(onClick = { removing = null }) { Text("やめる") } }
        )
    }
}

@Composable
private fun MenuRow(label: String, danger: Boolean = false, onClick: () -> Unit) {
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

/**
 * プロジェクト 1 枚。**状態 1 行 ＋ 進捗 ＋ 次の一手。**
 * どれを押せばいいかが、カードを見ただけで決まるようにする。
 */
@Composable
private fun ProjectCard(
    project: Project,
    standing: Standing?,
    /** 見本の 1 枚。無ければ出所のアイコンだけ出す。 */
    cover: Any?,
    onOpen: () -> Unit,
    onMenu: () -> Unit
) {
    Column(
        Modifier
            .clip(RoundedCornerShape(14.dp))
            .background(Surface)
            .clickable(onClick = onOpen)
            .padding(16.dp)
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            // **どのフォルダだったかを思い出すための 1 枚。**
            // 名前と枚数だけでは、開いて確かめることになる。
            Box(
                Modifier
                    .size(52.dp)
                    .clip(RoundedCornerShape(10.dp))
                    .background(Tile),
                contentAlignment = Alignment.Center
            ) {
                if (cover != null) {
                    AsyncImage(
                        model = ImageRequest.Builder(LocalContext.current)
                            .data(cover).size(160).build(),
                        contentDescription = null,
                        imageLoader = Images.loader(LocalContext.current),
                        contentScale = ContentScale.Crop,
                        modifier = Modifier.fillMaxSize()
                    )
                } else {
                    // 見本が無いときは出所を薄く。**空白のまま置かない。**
                    Icon(
                        if (project.source.kind == "nas") Icons.Filled.Dns
                        else Icons.Filled.Smartphone,
                        null, Modifier.size(18.dp), tint = Color(0xFF3A3E47)
                    )
                }
            }
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                Text(project.name, fontSize = 17.sp, fontWeight = FontWeight.SemiBold)
                // 出所は人の言葉で。生パスは技術情報だけに出す。
                Text(
                    project.source.label,
                    fontSize = 12.sp, color = Faint,
                    modifier = Modifier.padding(top = 2.dp)
                )
            }
            IconButton(onClick = onMenu, modifier = Modifier.size(28.dp)) {
                Icon(Icons.Filled.MoreVert, "その他", Modifier.size(18.dp), tint = Faint)
            }
        }

        Spacer(Modifier.height(10.dp))
        Text(
            standing?.line ?: "…",
            fontSize = 13.sp, fontWeight = FontWeight.SemiBold,
            color = standing?.tint ?: Faint
        )
        Spacer(Modifier.height(6.dp))
        val stars = standing?.stars
        if (stars != null && stars.sum() > 0) {
            // **完了したカードは星の内訳。** 進み具合はもう関係ない。
            Row(Modifier.fillMaxWidth().height(3.dp).clip(RoundedCornerShape(2.dp))) {
                val order = listOf(5, 4, 3, 2, 1, 0)
                for (star in order) {
                    val count = stars.getOrElse(star) { 0 }
                    if (count == 0) continue
                    Box(
                        Modifier
                            .weight(count.toFloat())
                            .fillMaxHeight()
                            .background(starTint(star))
                    )
                }
            }
        } else {
            Box(
                Modifier.fillMaxWidth().height(3.dp).clip(RoundedCornerShape(2.dp))
                    .background(Color(0xFF24272D))
            ) {
                val progress = standing?.progress
                if (progress != null && standing != null) {
                    Box(
                        Modifier.fillMaxWidth(progress.coerceIn(0f, 1f)).fillMaxHeight()
                            .clip(RoundedCornerShape(2.dp))
                            .background(standing.tint)
                    )
                }
            }
        }
        Row(
            Modifier.fillMaxWidth().padding(top = 10.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Text(standing?.detail ?: "", fontSize = 12.sp, color = Faint)
            Spacer(Modifier.weight(1f))
            Button(
                onClick = onOpen,
                shape = RoundedCornerShape(50),
                contentPadding = PaddingValues(horizontal = 16.dp)
            ) {
                Icon(Icons.Filled.PlayArrow, null, Modifier.size(16.dp))
                Spacer(Modifier.width(4.dp))
                Text(standing?.action ?: "開く", fontWeight = FontWeight.Bold, fontSize = 13.sp)
            }
        }
    }
}

/**
 * カードに出す状態を組み立てる。**保存されているものだけから決める。**
 *
 * ここで網へ行かない。ホームは一覧が出るまでの時間がすべてなので、
 * 「開いたら NAS を待つ」は作らない。準備の進みは端末に置いたもの
 * （顔ぶれ・指紋・表示用画像）を数えれば分かる。
 */
private suspend fun standingOf(context: Context, project: Project): Standing {
    val session = Store.load(context, project.id)

    // ---- つまずき ----
    // 準備で転んだ側が書き残したもの。**次に通れば消える。**
    val trouble = Trouble.load(context, project.source.key)
    if (trouble != null && session?.finished != true) {
        return Standing(trouble, "", null, Warn, "再試行")
    }

    // ---- 準備中 ----
    val known = Listing.load(context, project.source.key)
    if (session == null) {
        if (known == null) {
            // 一度も数えていない。**開けば走査が始まる。**
            return Standing("準備中 · 写真の走査", "", null, Sky, "開く")
        }
        val prints = Fingerprints.load(context, project.source.key).size
        if (prints < known.size) {
            return Standing(
                "準備中 · 撮影時刻・サムネイル",
                "$prints / ${known.size} 枚", ratio(prints, known.size), Sky, "開始"
            )
        }
        if (project.source.kind == "nas") {
            val nasId = project.source.key.substringBefore("|")
            val edge = Prefs.projectEdge(context, project.id)
            val made = Renders.count(context, nasId, edge)
            if (made < known.size) {
                return Standing(
                    "準備中 · 表示用画像を作成",
                    "$made / ${known.size} 枚", ratio(made, known.size), Sky, "開始"
                )
            }
        }
        // ---- 未開始 ----
        return Standing("準備完了", "${known.size} 枚", null, Faint, "開始")
    }

    val total = session.ratings.size
    if (session.finished) {
        val kept = session.ratings.values.count { it > 0 }
        val stars = (0..5).map { star -> session.ratings.values.count { it == star } }
        // **完了は白。** 選別中（primary）と一目で分ける。
        return Standing(
            "選別完了 · ★1 以上が $kept 枚", "$total 枚", null, Color.White, "結果を見る",
            stars = stars
        )
    }
    val remainingPhotos = (session.queue + session.current)
        .sumOf { session.members[it]?.size ?: 1 }
    val seen = session.history.sumOf { decision ->
        decision.group.sumOf { session.members[it]?.size ?: 1 }
    }
    return Standing(
        line = "★${session.targetStar} を選別中 · ROUND ${session.round}",
        detail = "残り $remainingPhotos / $total 枚",
        progress = if (total > 0) seen.toFloat() / total else null,
        tint = Lime,
        action = "続ける"
    )
}

/** 進み具合。**総数が 0 のときは棒を出さない**（0/0 は 100% ではない）。 */
private fun ratio(done: Int, total: Int): Float? =
    if (total > 0) (done.toFloat() / total).coerceIn(0f, 1f) else null

/** 星の色。**結果・ラウンド完了・ホームで同じ**にしておく。 */
internal fun starTint(star: Int): Color = when (star) {
    5 -> Lime
    4 -> Color(0xFFB7D95F)
    3 -> Color(0xFF8FA84F)
    2, 1 -> Color(0xFF5F6A3A)
    else -> Color(0xFF2A2D34)
}
