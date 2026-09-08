package app.photocurator.next

import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.rememberTransformableState
import androidx.compose.foundation.gestures.transformable
import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import coil.compose.AsyncImage
import coil.request.ImageRequest

/**
 * 1 枚を大きく見る。**ぶれとピントを確かめるための画面。**
 *
 * 選別は小さく並べて速く決める画面なので、迷ったときにだけここへ来る。
 * つまみで広げられて、二本指で動かせて、閉じたらすぐ元の並びに戻る。
 * ここで星は動かない。**見るだけの場所は、見るだけにする。**
 */
@Composable
fun ZoomView(photo: Photo, onClose: () -> Unit) {
    var scale by remember { mutableStateOf(1f) }
    var offsetX by remember { mutableStateOf(0f) }
    var offsetY by remember { mutableStateOf(0f) }

    fun reset() {
        scale = 1f; offsetX = 0f; offsetY = 0f
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
    ) {
        AsyncImage(
            model = ImageRequest.Builder(LocalContext.current)
                .data(photo.fullModel)
                // 拡大して見る画面なので、並びより大きく読む。
                .size(2048)
                .build(),
            contentDescription = photo.name,
            imageLoader = Images.loader(LocalContext.current),
            contentScale = ContentScale.Fit,
            modifier = Modifier
                .fillMaxSize()
                .graphicsLayer(
                    scaleX = scale, scaleY = scale,
                    translationX = offsetX, translationY = offsetY
                )
        )

        Row(
            Modifier
                .fillMaxWidth()
                .windowInsetsPadding(WindowInsets.safeDrawing)
                .padding(horizontal = 4.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            IconButton(onClick = onClose) {
                Icon(Icons.Filled.Close, "閉じる", tint = Color.White)
            }
            Text(
                photo.name,
                color = Color.White,
                fontSize = 12.sp,
                modifier = Modifier.weight(1f)
            )
            if (scale > 1.01f) {
                Text(
                    "${"%.1f".format(scale)}倍",
                    color = Lime, fontSize = 12.sp,
                    modifier = Modifier.padding(end = 12.dp)
                )
            }
        }
    }
}
