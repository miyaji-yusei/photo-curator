package app.photocurator.next

import androidx.activity.result.IntentSenderRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material.icons.filled.Check
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
 * ある星の写真を並べる。**選別の答え合わせをする場所。**
 *
 * ここから「取り出す」（お気に入りを付ける・別のアルバムへ移す）へ進む。
 * 取り出しは**原本に手を入れる**ので、この画面は選ぶところまでを受け持ち、
 * 実際の書き込みは Take が端末の同意を取ってから行う。
 */
@Composable
fun ResultsScreen(project: Project, star: Int, onBack: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()

    var photos by remember { mutableStateOf<List<Photo>>(emptyList()) }
    var picked by remember { mutableStateOf<Set<String>>(emptySet()) }
    var note by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    var reloads by remember { mutableStateOf(0) }
    // 同意画面へ出した枚数。戻ってきたときに何枚の話だったかを言うため。
    var pending by remember { mutableStateOf(0) }
    // 取り出す前に自分で確認を出す。理由は下の AlertDialog に書いた。
    var confirming by remember { mutableStateOf<List<Photo>?>(null) }
    // 移し先を選んでいる最中か。
    var choosingDestination by remember { mutableStateOf(false) }
    // 移す確認を出している相手（行き先と枚数）。
    var confirmingMove by remember { mutableStateOf<Album?>(null) }
    // 許可が下りたあと、どこへ何枚移すかを覚えておく。
    var pendingMove by remember { mutableStateOf<Pair<Album, List<Photo>>?>(null) }

    // 取り出しの同意は OS の画面で取る。**戻ってきた結果を必ず言葉にする。**
    val consent = rememberLauncherForActivityResult(
        ActivityResultContracts.StartIntentSenderForResult()
    ) { result ->
        val move = pendingMove
        if (move == null) {
            note = Take.describe(result.resultCode, pending)
            busy = false
            picked = emptySet()
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
                    picked = emptySet()
                    reloads += 1
                }
            }
        }
    }

    LaunchedEffect(project.id, star, reloads) {
        val all = Photos.forSource(context, project.source)
        val session = Store.load(context, project.id)
        val ratings = session?.ratings ?: emptyMap()
        // 星が付いていないものは 0 として扱う。**未知を「無い」にしない。**
        photos = all.filter { (ratings[it.relativePath] ?: 0) == star }
        picked = emptySet()
    }

    Column(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing)) {
        Reading {
        Row(
            Modifier.fillMaxWidth().height(56.dp).padding(end = 12.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            IconButton(onClick = onBack) { Icon(Icons.Filled.ArrowBack, "戻る") }
            Column(Modifier.weight(1f)) {
                Text("★$star", fontSize = 15.sp, fontWeight = FontWeight.SemiBold, color = Lime)
                Text("${photos.size} 枚", fontSize = 12.sp, color = Faint)
            }
            if (picked.isEmpty()) {
                TextButton(
                    onClick = { picked = photos.map { it.relativePath }.toSet() },
                    enabled = photos.isNotEmpty()
                ) { Text("すべて選ぶ", fontSize = 13.sp) }
            } else {
                TextButton(onClick = { picked = emptySet() }) {
                    Text("解除", fontSize = 13.sp)
                }
            }
        }
        }

        note?.let {
            Text(
                it, fontSize = 12.sp, color = Faint,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp)
            )
        }

        if (photos.isEmpty()) {
            Box(Modifier.weight(1f), contentAlignment = Alignment.Center) {
                Text("この星の写真はありません", color = Faint, fontSize = 13.sp)
            }
        } else {
            LazyVerticalGrid(
                columns = GridCells.Adaptive(minSize = 108.dp),
                modifier = Modifier.weight(1f),
                contentPadding = PaddingValues(4.dp)
            ) {
                items(photos, key = { it.id }) { photo ->
                    val on = photo.relativePath in picked
                    Box(
                        Modifier
                            .padding(2.dp)
                            .aspectRatio(1f)
                            .clip(RoundedCornerShape(6.dp))
                            .background(Tile)
                            .then(if (on) Modifier.border(3.dp, Lime, RoundedCornerShape(6.dp)) else Modifier)
                            .clickable {
                                picked = if (on) picked - photo.relativePath
                                else picked + photo.relativePath
                            }
                    ) {
                        AsyncImage(
                            model = ImageRequest.Builder(LocalContext.current)
                                .data(photo.uri).size(320).build(),
                            contentDescription = photo.name,
                            contentScale = ContentScale.Crop,
                            modifier = Modifier.fillMaxSize()
                        )
                        if (on) {
                            Box(
                                Modifier
                                    .align(Alignment.TopEnd)
                                    .padding(4.dp)
                                    .clip(RoundedCornerShape(50))
                                    .background(Lime)
                                    .padding(2.dp)
                            ) {
                                Icon(
                                    Icons.Filled.Check, null,
                                    Modifier.size(14.dp), tint = Color.Black
                                )
                            }
                        }
                    }
                }
            }
        }

        // ---- 取り出す ----
        // **原本に触る操作なので、何枚に何をするかを文字で見せてから押させる。**
        if (picked.isNotEmpty()) {
            Reading(Modifier.padding(12.dp)) {
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(8.dp)
            ) {
                OutlinedButton(
                    onClick = { confirming = photos.filter { it.relativePath in picked } },
                    enabled = !busy,
                    shape = RoundedCornerShape(50),
                    modifier = Modifier.weight(1f).height(46.dp)
                ) {
                    Text("${picked.size} 枚をお気に入りに", fontSize = 13.sp)
                }
                OutlinedButton(
                    onClick = { choosingDestination = true },
                    enabled = !busy,
                    shape = RoundedCornerShape(50),
                    modifier = Modifier.weight(1f).height(46.dp)
                ) {
                    Text("別のアルバムへ移す", fontSize = 13.sp)
                }
            }
            }
        }
    }

    // **写真そのものに触る前に、必ずここを通す。**
    //
    // OS の同意画面（createFavoriteRequest）は、いつも出るとは限らない。
    // すでにその写真への許可を持っていると MediaProvider は黙って通す。
    // 実機で確認したとき、画面が出ないまま 4 枚に印が付いた。
    // 確認を OS に預けたままにすると、**押した覚えのない書き込みが起きる。**
    confirming?.let { targets ->
        AlertDialog(
            onDismissRequest = { confirming = null },
            title = { Text("${targets.size} 枚にお気に入りを付けます") },
            text = {
                Text(
                    "端末の写真に印が付きます（Google フォトなどからも見えます）。" +
                        "写真そのものは動かしませんし、消えません。後から外せます。"
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    confirming = null
                    val sender = Take.favouriteRequest(context, targets)
                    if (sender == null) {
                        // **何も起きないまま終わらせない。** 対応していない端末なのか、
                        // 要求を作れなかったのかを言う。
                        note = "この端末ではお気に入りを付けられません"
                    } else {
                        pending = targets.size
                        busy = true
                        consent.launch(IntentSenderRequest.Builder(sender).build())
                    }
                }) { Text("付ける") }
            },
            dismissButton = {
                TextButton(onClick = { confirming = null }) { Text("やめる") }
            }
        )
    }
    // 移し先を選ぶ。ここではまだ何も書かない。
    if (choosingDestination) {
        DestinationSheet(
            from = project,
            count = picked.size,
            onPick = { destination ->
                choosingDestination = false
                confirmingMove = destination
            },
            onDismiss = { choosingDestination = false }
        )
    }

    // **移すのは戻しにくい。** お気に入りより強い言い方で確かめる。
    // OS の同意画面は出ないことがあるので、ここが最後の関門になる。
    confirmingMove?.let { destination ->
        val targets = photos.filter { it.relativePath in picked }
        AlertDialog(
            onDismissRequest = { confirmingMove = null },
            title = { Text("${targets.size} 枚を「${destination.name}」へ移します") },
            text = {
                Text(
                    "端末の中の置き場所が ${destination.relativeDir} に変わります。" +
                        "写真は消えませんが、元のアルバムからは無くなります。" +
                        "この操作にアプリ側の取り消しはありません。"
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    confirmingMove = null
                    val sender = Take.moveRequest(context, targets)
                    if (sender == null) {
                        note = "この端末では移せません"
                    } else {
                        pendingMove = destination to targets
                        busy = true
                        consent.launch(IntentSenderRequest.Builder(sender).build())
                    }
                }) { Text("移す") }
            },
            dismissButton = {
                TextButton(onClick = { confirmingMove = null }) { Text("やめる") }
            }
        )
    }
}
