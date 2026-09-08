package app.photocurator.next

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.rememberTransformableState
import androidx.compose.foundation.gestures.transformable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ChevronLeft
import androidx.compose.material.icons.filled.ChevronRight
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import coil.compose.AsyncImage
import coil.compose.AsyncImagePainter
import coil.compose.rememberAsyncImagePainter
import coil.request.ImageRequest

/**
 * 1 枚を大きく見る。**ぶれとピントを確かめるための画面。**
 *
 * ここだけは**原本を読む**。ただし NAS の原本は 1 枚 6MB あって網越しでは
 * すぐ出ない。だから**表示用画像を先に出して、原本が届いたら黙って差し替える**。
 * 白い画面で待たせない。読めなければ表示用画像のまま、そう書いて出し続ける。
 *
 * 前後に動ける。迷う 1 枚は隣と見比べないと決まらないので、
 * **閉じて開き直さずに横へ**送れるようにする。
 */
@Composable
fun ZoomView(
    /** 前後に送れる並び。1 枚だけ渡してもよい。 */
    photos: List<Photo>,
    startAt: Int,
    displayEdge: Int,
    /** 「残す」を出すか。**選別から来たときだけ**。見るだけの場所では出さない。 */
    onKeep: ((Photo) -> Unit)? = null,
    onClose: () -> Unit
) {
    if (photos.isEmpty()) {
        onClose()
        return
    }

    var at by remember { mutableStateOf(startAt.coerceIn(0, photos.lastIndex)) }
    val photo = photos[at]

    var scale by remember { mutableStateOf(1f) }
    var offsetX by remember { mutableStateOf(0f) }
    var offsetY by remember { mutableStateOf(0f) }

    fun reset() {
        scale = 1f; offsetX = 0f; offsetY = 0f
    }

    // 隣へ動いたら倍率も位置も戻す。**前の 1 枚の見え方を持ち越さない。**
    fun move(step: Int) {
        val next = at + step
        if (next in photos.indices) {
            at = next
            reset()
        }
    }

    val transform = rememberTransformableState { zoomChange, panChange, _ ->
        // 1 倍より小さくしない。**縮めても情報は増えない**し、指を離した
        // ときにどこにいるのか分からなくなる。
        scale = (scale * zoomChange).coerceIn(1f, 6f)
        if (scale > 1f) {
            offsetX += panChange.x
            offsetY += panChange.y
        } else {
            // 等倍に戻ったら位置も戻す。ずれたまま残ると次が見づらい。
            offsetX = 0f; offsetY = 0f
        }
    }

    val context = LocalContext.current
    // 原本。**届くまで state は Loading のまま**なので、それを文にして出す。
    val full = rememberAsyncImagePainter(
        model = ImageRequest.Builder(context).data(photo.fullModel).size(2048).build(),
        imageLoader = Images.loader(context)
    )
    val ready = full.state is AsyncImagePainter.State.Success
    val broken = full.state is AsyncImagePainter.State.Error

    Box(
        Modifier
            .fillMaxSize()
            .background(Color.Black)
            .transformable(transform)
            .pointerInput(photo.id) {
                detectTapGestures(
                    // 等倍なら閉じる。広げているなら等倍に戻す。
                    // **「閉じる」と「戻す」を 1 つの動作に混ぜない。**
                    onTap = { if (scale > 1.01f) reset() else onClose() },
                    onDoubleTap = { if (scale > 1.01f) reset() else scale = 2.5f }
                )
            }
            .pointerInput(photos, at) {
                // **広げているあいだは横送りしない。** 拡大中の指の動きは
                // 位置合わせであって、隣へ行きたいわけではない。
                var swept = 0f
                detectHorizontalDragGestures(
                    onDragEnd = {
                        if (scale <= 1.01f && kotlin.math.abs(swept) > 80f) {
                            move(if (swept < 0) 1 else -1)
                        }
                        swept = 0f
                    }
                ) { _, amount -> swept += amount }
            }
    ) {
        // ---- 下: 表示用画像。**必ず出る絵。** ----
        AsyncImage(
            model = ImageRequest.Builder(context)
                .data(photo.displayModel(displayEdge)).size(displayEdge).build(),
            contentDescription = photo.name,
            imageLoader = Images.loader(context),
            contentScale = ContentScale.Fit,
            modifier = Modifier
                .fillMaxSize()
                .graphicsLayer(
                    scaleX = scale, scaleY = scale,
                    translationX = offsetX, translationY = offsetY
                )
        )

        // ---- 上: 原本。届いたら**黙って**重なる。 ----
        if (ready) {
            Image(
                painter = full,
                contentDescription = photo.name,
                contentScale = ContentScale.Fit,
                modifier = Modifier
                    .fillMaxSize()
                    .graphicsLayer(
                        scaleX = scale, scaleY = scale,
                        translationX = offsetX, translationY = offsetY
                    )
            )
        }

        // ---- 上端: 閉じる・名前・いま何枚目か ----
        // **写真の上に文字を直接置かない。** 白い写真だと白文字が消える。
        // 実機で「原本を読み込み中…」がまったく読めなかった。
        Row(
            Modifier
                .fillMaxWidth()
                .background(
                    Brush.verticalGradient(
                        listOf(Color(0xCC000000), Color(0x00000000))
                    )
                )
                .windowInsetsPadding(WindowInsets.safeDrawing)
                .padding(horizontal = 4.dp, vertical = 6.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            IconButton(onClick = onClose) {
                Icon(Icons.Filled.Close, "閉じる", tint = Color.White)
            }
            Column(Modifier.weight(1f)) {
                Text(photo.name, color = Color.White, fontSize = 12.sp)
                // **いま何を見せているのかを隠さない。**
                Text(
                    when {
                        ready -> "原本"
                        broken -> "原本を読めません · 表示用画像で表示中"
                        else -> "原本を読み込み中 ${megabytes(photo.size)} · 表示用画像で先に表示"
                    },
                    color = if (broken) Color(0xFFFF8A80) else Color(0xCCFFFFFF),
                    fontSize = 11.sp
                )
            }
            if (photos.size > 1) {
                Text(
                    "${at + 1} / ${photos.size}",
                    color = Color(0xCCFFFFFF), fontSize = 12.sp,
                    modifier = Modifier.padding(end = 8.dp)
                )
            }
            if (scale > 1.01f) {
                Text(
                    "${"%.1f".format(scale)}倍",
                    color = Lime, fontSize = 12.sp,
                    modifier = Modifier.padding(end = 12.dp)
                )
            }
        }

        // ---- 左右: 前後へ。**指が滑らない人のために押せる場所も置く。** ----
        if (photos.size > 1 && scale <= 1.01f) {
            if (at > 0) {
                Arrow(Icons.Filled.ChevronLeft, "前の写真", Alignment.CenterStart) { move(-1) }
            }
            if (at < photos.lastIndex) {
                Arrow(Icons.Filled.ChevronRight, "次の写真", Alignment.CenterEnd) { move(1) }
            }
        }

        // ---- 下端: 残す ----
        // **見るだけの場所からは出さない。** 詳細と結果ではここに何も出ない。
        if (onKeep != null) {
            Row(
                Modifier
                    .align(Alignment.BottomCenter)
                    .windowInsetsPadding(WindowInsets.safeDrawing)
                    .padding(bottom = 20.dp)
            ) {
                Button(
                    onClick = { onKeep(photo) },
                    shape = RoundedCornerShape(50)
                ) { Text("この 1 枚を残す", fontWeight = FontWeight.Bold) }
            }
        }
    }
}

@Composable
private fun BoxScope.Arrow(
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    label: String,
    where: Alignment,
    onClick: () -> Unit
) {
    Box(
        Modifier
            .align(where)
            .padding(horizontal = 8.dp)
            .size(52.dp)
            .clip(RoundedCornerShape(26.dp))
            .background(Color(0x66000000)),
        contentAlignment = Alignment.Center
    ) {
        IconButton(onClick = onClick) {
            Icon(icon, label, Modifier.size(30.dp), tint = Color.White)
        }
    }
}

/** 「6.7MB」。**原本の重さは、待たされる理由そのもの**なので数字で出す。 */
private fun megabytes(bytes: Long): String =
    if (bytes <= 0) "サイズ不明" else "%.1fMB".format(bytes / 1024.0 / 1024.0)
