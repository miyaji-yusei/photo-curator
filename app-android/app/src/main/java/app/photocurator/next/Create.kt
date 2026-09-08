@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package app.photocurator.next

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Folder
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Visibility
import androidx.compose.material.icons.filled.VisibilityOff
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

/**
 * プロジェクトを作る。**出所とフォルダを選ぶ。**
 *
 * 「いま何を見ているか」を常に 1 か所（出所タブ）で示す。
 * タブが下の一覧の出所と 1 対 1 になっていて、**切り替えたら必ず取り直す**。
 * これがずれると、別の場所のフォルダを選んだつもりで作ってしまう。
 *
 * **画面にする。シートにしない。** 下から出るシートだと、フォルダ一覧を
 * 送ろうとした指がシートごと下へ動かして閉じてしまう。フォルダが多いほど
 * 起きやすく、選び直しからやり直すことになる。
 */
@Composable
fun CreateScreen(onCreated: (Project) -> Unit, onDismiss: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()

    // いまは「この端末」だけ。NAS のタブは繋げられるようになってから足す。
    var tab by remember { mutableStateOf("album") }
    var albums by remember { mutableStateOf<List<Album>>(emptyList()) }
    var taken by remember { mutableStateOf<Set<String>>(emptySet()) }
    var note by remember { mutableStateOf("読み込み中…") }
    var chosen by remember { mutableStateOf<Album?>(null) }
    var name by remember { mutableStateOf("") }
    // 登録ずみの NAS。タブはここから作る。
    var nasList by remember { mutableStateOf<List<Nas>>(emptyList()) }
    var folders by remember { mutableStateOf<List<SmbFolder>>(emptyList()) }
    var chosenFolder by remember { mutableStateOf<SmbFolder?>(null) }
    // パスワードを聞く必要があるつなぎ先。**保存していない人のための道。**
    var asking by remember { mutableStateOf<Nas?>(null) }
    // 選んだフォルダの中身を少しだけ。**選ぶ前のフォルダは取りに行かない。**
    // いま開いている NAS のフォルダ（"" は共有の直下）。**潜れるようにする。**
    // 直下に写真が無くても、中のフォルダに写真があることは普通にある。
    var here by remember { mutableStateOf("") }
    var strip by remember { mutableStateOf<List<Any>>(emptyList()) }
    // 取れた見本。**行の再構成はこれで起こす。**
    // SmbFolder は取得の前後で同じ値なので、一覧を作り直しても行は更新されない
    // （Compose は入力が等しければ省く）。取れたことを別の状態で伝える。
    var covers by remember { mutableStateOf<Map<String, java.io.File>>(emptyMap()) }
    var stripNote by remember { mutableStateOf("") }

    LaunchedEffect(Unit) { nasList = NasStore.all(context) }

    // 選んだフォルダの中身を少しだけ出す。**選んだものだけ、最大 12 枚。**
    LaunchedEffect(chosen?.id, chosenFolder?.path, tab) {
        strip = emptyList()
        stripNote = ""
        val album = chosen
        val folder = chosenFolder
        when {
            album != null -> {
                // 端末は手元なので、そのまま並べてよい。
                val inside = Photos.forSource(context, Source("album", album.name, album.id))
                strip = inside.take(Covers.STRIP).map { it.thumbModel }
                stripNote = if (inside.size > Covers.STRIP)
                    "先頭の ${Covers.STRIP} 枚" else ""
            }
            folder != null -> {
                val nas = nasList.firstOrNull { it.id == tab } ?: return@LaunchedEffect
                val password = Session.password(context, nas) ?: return@LaunchedEffect
                stripNote = "中身を読み込んでいます…"
                val inside = (Smb.photos(nas, password, folder.path) as? SmbResult.Ok)?.value
                if (inside == null) {
                    // **選ぶ前の画面で失敗を並べない。** 出せなければ黙って出さない。
                    stripNote = ""
                    return@LaunchedEffect
                }
                val wanted = inside.take(Covers.STRIP).map { it.path }
                Covers.warm(context, nas, password, wanted)
                strip = wanted
                    .filter { ThumbCache.has(context, nas.id, it) }
                    .map { ThumbCache.file(context, nas.id, it) }
                stripNote = when {
                    strip.isEmpty() -> ""
                    inside.size > Covers.STRIP -> "先頭の ${strip.size} 枚"
                    else -> ""
                }
            }
        }
    }

    // **タブが変わったら必ず取り直す。** 前のタブの一覧が残っていると、
    // 見出しと中身が食い違う。
    LaunchedEffect(tab, nasList, here) {
        albums = emptyList()
        folders = emptyList()
        covers = emptyMap()
        chosen = null
        chosenFolder = null
        name = ""
        note = "読み込み中…"
        if (tab == "album") {
            albums = Photos.albums(context)
            taken = Projects.all(context)
                .filter { it.source.kind == "album" }
                .map { it.source.key }
                .toSet()
            note = if (albums.isEmpty()) "この端末に写真のフォルダがありません" else ""
            return@LaunchedEffect
        }

        val nas = nasList.firstOrNull { it.id == tab } ?: return@LaunchedEffect
        val password = Session.password(context, nas)
        if (password == null) {
            // **黙って空にしない。** 何が足りないのかを言って、入れる道を出す。
            note = "パスワードが要ります"
            asking = nas
            return@LaunchedEffect
        }
        note = "${nas.label} に接続しています…"
        when (val answer = Smb.folders(nas, password, here)) {
            is SmbResult.Ok -> {
                folders = answer.value
                taken = Projects.all(context)
                    .filter { it.source.kind == "nas" }
                    .map { it.source.key }
                    .toSet()
                note = if (folders.isEmpty())
                    "${nas.share} に写真のフォルダがありません" else ""
                // 見本を**1 本の接続でまとめて**取る。1 枚ずつ繋ぎ直すと
                // 1 枚 800ms かかる（実測）。取れた分は端末に残るので次は速い。
                Covers.warm(context, nas, password, answer.value.mapNotNull { it.cover })
                covers = answer.value.mapNotNull { folder ->
                    val path = folder.cover ?: return@mapNotNull null
                    ThumbCache.file(context, nas.id, path)
                        .takeIf { it.exists() && it.length() > 0 }
                        ?.let { folder.path to it }
                }.toMap()
            }
            is SmbResult.Failed -> note = answer.reason
        }
    }

    Column(
        Modifier
            .fillMaxSize()
            .windowInsetsPadding(WindowInsets.safeDrawing)
            .padding(horizontal = 16.dp)
            .padding(bottom = 16.dp)
    ) {
        Row(Modifier.height(64.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onDismiss) { Icon(Icons.Filled.ArrowBack, "戻る") }
            Spacer(Modifier.width(4.dp))
            Text("プロジェクトを作成", fontSize = 18.sp, fontWeight = FontWeight.SemiBold)
        }
        // 中身は残り全部を使う。**一覧が長いほど、送れる面積が要る。**
        Column(Modifier.weight(1f)) {

            // ---- 出所タブ ----
            Row(
                Modifier.padding(top = 4.dp, bottom = 12.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp)
            ) {
                FilterChip(
                    selected = tab == "album",
                    onClick = { tab = "album" },
                    label = { Text("この端末", fontSize = 13.sp) },
                    colors = FilterChipDefaults.filterChipColors(
                        selectedContainerColor = Lime, selectedLabelColor = Color.Black
                    )
                )
                for (nas in nasList) {
                    FilterChip(
                        selected = tab == nas.id,
                        onClick = { tab = nas.id },
                        // **人の言葉で。** 「home-nas · Share」
                        label = { Text("${nas.label} · ${nas.share}", fontSize = 13.sp) },
                        colors = FilterChipDefaults.filterChipColors(
                            selectedContainerColor = Lime, selectedLabelColor = Color.Black
                        )
                    )
                }
            }

            Row(Modifier.weight(1f)) {
                // ---- 左: フォルダ一覧 ----
                Column(Modifier.weight(1f)) {
                    Row(
                        Modifier.fillMaxWidth().padding(bottom = 6.dp),
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        // **いまどこを見ているかを出す。** 潜ったまま迷子にしない。
                        if (tab != "album" && here.isNotEmpty()) {
                            TextButton(onClick = {
                                here = here.substringBeforeLast('\\', "")
                                chosenFolder = null
                            }) { Text("← 上へ", fontSize = 12.sp) }
                            Spacer(Modifier.width(8.dp))
                        }
                        Text(
                            when {
                                tab == "album" -> "この端末のフォルダ · ${albums.size} 件"
                                here.isEmpty() ->
                                    "${nasList.firstOrNull { it.id == tab }?.share ?: ""} のフォルダ · ${folders.size} 件"
                                else -> here.replace('\\', '/') + " · ${folders.size} 件"
                            },
                            fontSize = 12.sp, color = Faint
                        )
                    }
                    if (note.isNotEmpty()) {
                        Text(
                            note, fontSize = 13.sp, color = Faint,
                            modifier = Modifier.padding(vertical = 24.dp)
                        )
                    }
                    LazyColumn {
                        items(folders, key = { it.path }) { folder ->
                            val chosenHere = chosenFolder?.path == folder.path
                            val nasId = tab
                            val already = "$nasId|${folder.path}" in taken
                            Row(
                                Modifier
                                    .fillMaxWidth()
                                    .clip(RoundedCornerShape(10.dp))
                                    .then(
                                        if (chosenHere) Modifier
                                            .background(Color(0x24D6FF73))
                                            .border(1.dp, Lime, RoundedCornerShape(10.dp))
                                        else Modifier
                                    )
                                    .clickable {
                                        // 写真が無いフォルダは選べない（中へ入る）。
                                        if (folder.count == 0 && folder.folders > 0) {
                                            here = folder.path
                                            return@clickable
                                        }
                                        chosenFolder = folder
                                        chosen = null
                                        // 名前の既定はフォルダ名。**ID や道筋は入れない。**
                                        name = folder.name
                                    }
                                    .padding(horizontal = 10.dp, vertical = 12.dp),
                                verticalAlignment = Alignment.CenterVertically
                            ) {
                                FolderCover(covers[folder.path])
                                Spacer(Modifier.width(10.dp))
                                Text(folder.name, fontSize = 14.sp, modifier = Modifier.weight(1f))
                                if (already) Text("作成済み", fontSize = 12.sp, color = Faint)
                                else if (folder.count > 0) {
                                    Text("${folder.count} 枚", fontSize = 12.sp, color = Faint)
                                }
                                if (chosenHere) {
                                    Spacer(Modifier.width(8.dp))
                                    Icon(Icons.Filled.Check, null, Modifier.size(16.dp), tint = Lime)
                                }
                                // **中にフォルダがあれば入れる。** 写真が無くても
                                // 入り口を出す（入れ子に置いた写真へ辿り着けるように）。
                                if (folder.folders > 0) {
                                    Spacer(Modifier.width(8.dp))
                                    TextButton(onClick = { here = folder.path; chosenFolder = null }) {
                                        Text("中へ（${folder.folders}）", fontSize = 12.sp)
                                    }
                                }
                            }
                        }
                        items(albums, key = { it.id }) { album ->
                            val here = chosen?.id == album.id
                            val already = album.id in taken
                            Row(
                                Modifier
                                    .fillMaxWidth()
                                    .clip(RoundedCornerShape(10.dp))
                                    .then(
                                        if (here) Modifier
                                            .background(Color(0x24D6FF73))
                                            .border(1.dp, Lime, RoundedCornerShape(10.dp))
                                        else Modifier
                                    )
                                    .clickable {
                                        chosen = album
                                        // 名前の既定はフォルダ名。**ID は入れない。**
                                        name = album.name
                                    }
                                    .padding(horizontal = 10.dp, vertical = 12.dp),
                                verticalAlignment = Alignment.CenterVertically
                            ) {
                                FolderCover(
                                    android.content.ContentUris.withAppendedId(
                                        android.provider.MediaStore.Images.Media.EXTERNAL_CONTENT_URI,
                                        album.coverId
                                    )
                                )
                                Spacer(Modifier.width(10.dp))
                                Text(album.name, fontSize = 14.sp, modifier = Modifier.weight(1f))
                                if (already) {
                                    Text("作成済み", fontSize = 12.sp, color = Faint)
                                } else {
                                    Text("${album.count} 枚", fontSize = 12.sp, color = Faint)
                                }
                                if (here) {
                                    Spacer(Modifier.width(8.dp))
                                    Icon(Icons.Filled.Check, null, Modifier.size(16.dp), tint = Lime)
                                }
                            }
                        }
                    }
                }

                Spacer(Modifier.width(16.dp))

                // ---- 右: 名前と出所の確認 ----
                Column(Modifier.width(260.dp)) {
                    OutlinedTextField(
                        value = name,
                        onValueChange = { name = it },
                        singleLine = true,
                        label = { Text("プロジェクト名") },
                        enabled = chosen != null || chosenFolder != null,
                        modifier = Modifier.fillMaxWidth()
                    )
                    Text(
                        "フォルダ名を使います。あとで変えられます",
                        fontSize = 11.sp, color = Faint,
                        modifier = Modifier.padding(top = 4.dp, bottom = 14.dp)
                    )
                    chosenFolder?.let { folder ->
                        val nas = nasList.firstOrNull { it.id == tab }
                        Confirm("出所", nas?.label ?: "NAS")
                        // **どこにあるフォルダなのかを実際の道筋で見せる。**
                        Confirm("フォルダ", (nas?.share ?: "") + " / " + folder.path)
                        Confirm("写真", "${folder.count} 枚")
                        Spacer(Modifier.height(12.dp))
                        Row(verticalAlignment = Alignment.Top) {
                            Icon(Icons.Filled.Lock, null, Modifier.size(14.dp), tint = Faint)
                            Spacer(Modifier.width(6.dp))
                            Text(
                                "原本は読むだけ。端末にコピーせず、必要な部分だけ読みます",
                                fontSize = 11.sp, color = Faint
                            )
                        }
                    }
                    // **中身を少しだけ見せる。** 名前と枚数だけでは決められない。
                    if (strip.isNotEmpty() || stripNote.isNotEmpty()) {
                        Spacer(Modifier.height(14.dp))
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Text("中身", fontSize = 12.sp, color = Faint)
                            Spacer(Modifier.weight(1f))
                            if (stripNote.isNotEmpty()) {
                                Text(stripNote, fontSize = 11.sp, color = Faint)
                            }
                        }
                        Spacer(Modifier.height(6.dp))
                        Row(
                            Modifier
                                .fillMaxWidth()
                                .horizontalScroll(rememberScrollState()),
                            horizontalArrangement = Arrangement.spacedBy(4.dp)
                        ) {
                            for (item in strip) {
                                Box(
                                    Modifier
                                        .size(64.dp)
                                        .clip(RoundedCornerShape(6.dp))
                                        .background(Tile)
                                ) {
                                    AsyncImage(
                                        model = ImageRequest.Builder(context)
                                            .data(item).size(160).build(),
                                        contentDescription = null,
                                        imageLoader = Images.loader(context),
                                        contentScale = ContentScale.Crop,
                                        modifier = Modifier.fillMaxSize()
                                    )
                                }
                            }
                        }
                    }

                    chosen?.let { album ->
                        Confirm("出所", "この端末")
                        Confirm("フォルダ", album.relativeDir.ifBlank { album.name })
                        Confirm("写真", "${album.count} 枚")
                        Spacer(Modifier.height(12.dp))
                        Row(verticalAlignment = Alignment.Top) {
                            Icon(Icons.Filled.Lock, null, Modifier.size(14.dp), tint = Faint)
                            Spacer(Modifier.width(6.dp))
                            Text(
                                "原本は読むだけ。星やまとめ方はこのアプリの中に保存します",
                                fontSize = 11.sp, color = Faint
                            )
                        }
                    }
                }
            }

            asking?.let { nas ->
                AskPassword(
                    nas = nas,
                    onEntered = { entered ->
                        Session.hold(nas.id, entered)
                        asking = null
                        // 同じタブをもう一度読み直させる。
                        nasList = nasList.toList()
                    },
                    onDismiss = { asking = null; tab = "album" }
                )
            }

            Row(
                Modifier.fillMaxWidth().padding(top = 16.dp),
                horizontalArrangement = Arrangement.End,
                verticalAlignment = Alignment.CenterVertically
            ) {
                TextButton(onClick = onDismiss) { Text("キャンセル") }
                Spacer(Modifier.width(8.dp))
                Button(
                    onClick = {
                        val folder = chosenFolder
                        val album = chosen
                        val nas = nasList.firstOrNull { it.id == tab }
                        scope.launch {
                            val project = when {
                                folder != null && nas != null -> Projects.add(
                                    context, name.trim().ifEmpty { folder.name },
                                    Source(
                                        kind = "nas",
                                        // **人の言葉。** 生の道筋は技術情報だけに出す。
                                        label = "${nas.label} · ${nas.share} / ${folder.name}",
                                        // 出所の鍵は「どの NAS の、どの道筋か」。
                                        key = "${nas.id}|${folder.path}"
                                    )
                                )
                                album != null -> Projects.add(
                                    context, name.trim().ifEmpty { album.name },
                                    Source(
                                        kind = "album",
                                        label = "この端末・アルバム「${album.name}」",
                                        key = album.id
                                    )
                                )
                                else -> null
                            }
                            project?.let(onCreated)
                        }
                    },

                    enabled = chosen != null || chosenFolder != null,
                    shape = RoundedCornerShape(50)
                ) {
                    Text("作成して準備を始める", fontWeight = FontWeight.Bold)
                }
            }
        }
    }
}

/**
 * つなぐたびに聞く。**保存トグルが off の人のための道。**
 * ここで入れた分は、このアプリを閉じるまでは覚えている。
 */
@Composable
private fun AskPassword(nas: Nas, onEntered: (String) -> Unit, onDismiss: () -> Unit) {
    var password by remember { mutableStateOf("") }
    var shown by remember { mutableStateOf(false) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("${nas.label} のパスワード") },
        text = {
            Column {
                Text(
                    "この端末には保存していない設定です。閉じるまで覚えています。",
                    fontSize = 12.sp, color = Faint
                )
                Spacer(Modifier.height(8.dp))
                OutlinedTextField(
                    value = password,
                    onValueChange = { password = it },
                    singleLine = true,
                    label = { Text("パスワード") },
                    visualTransformation =
                        if (shown) androidx.compose.ui.text.input.VisualTransformation.None
                        else androidx.compose.ui.text.input.PasswordVisualTransformation(),
                    trailingIcon = {
                        IconButton(onClick = { shown = !shown }) {
                            Icon(
                                if (shown) Icons.Filled.VisibilityOff else Icons.Filled.Visibility,
                                if (shown) "隠す" else "表示する"
                            )
                        }
                    }
                )
            }
        },
        confirmButton = {
            TextButton(onClick = { onEntered(password) }, enabled = password.isNotEmpty()) {
                Text("つなぐ")
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("やめる") } }
    )
}

@Composable
private fun Confirm(label: String, value: String) {
    Row(Modifier.fillMaxWidth().padding(vertical = 3.dp)) {
        Text(label, fontSize = 12.sp, color = Faint, modifier = Modifier.width(64.dp))
        Text(value, fontSize = 12.sp)
    }
}

/**
 * フォルダ行の見本。**無ければフォルダのアイコンに戻す。**
 * 選ぶ前の画面なので、読めなかったことを言葉で並べない。
 */
@Composable
private fun FolderCover(model: Any?) {
    Box(
        Modifier.size(36.dp).clip(RoundedCornerShape(8.dp)).background(Tile),
        contentAlignment = Alignment.Center
    ) {
        if (model == null) {
            Icon(Icons.Filled.Folder, null, Modifier.size(18.dp), tint = Faint)
        } else {
            AsyncImage(
                model = ImageRequest.Builder(LocalContext.current).data(model).size(120).build(),
                contentDescription = null,
                imageLoader = Images.loader(LocalContext.current),
                contentScale = ContentScale.Crop,
                modifier = Modifier.fillMaxSize()
            )
        }
    }
}
