package app.photocurator.next

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Favorite
import androidx.compose.material.icons.filled.Star
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import coil.compose.AsyncImage
import coil.request.ImageRequest
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.launch

/**
 * スライドショー選別（U19）。写真を 1 枚ずつ、枠いっぱいに出し、
 * 残す（右）／落とす（左）／★5 で確定（上）を決める。
 *
 * **判定は [SlideshowGesture]（PC・Web と同じ数値・規則）。ここは入力を集めて
 * 見せるだけ**で、決まった操作は呼び出し側が core（advance・keepAndTop）へ渡す。
 * 長押しの拡大はしない（拡大は右上のボタンだけ）。下向きは使わない。
 */
private const val MARGIN_DP = 6f
private const val FLY_MS = 220
private const val SNAP_MS = 180

private val DropColor = Color(0xFFEF5350)
private val KeepColor = Color(0xFF4CAF50)

@Composable
internal fun SlideshowStage(
    /** 出している 1 枚（代表）の相対パス。変わったら動きは最初からになる。 */
    path: String,
    photo: Photo?,
    /** この 1 枚が何枚ぶんの代表か（連写の印）。 */
    stands: Int,
    displayEdge: Int,
    onDecide: (SlideDecision) -> Unit,
    onZoom: () -> Unit,
    onOpenBurst: () -> Unit,
    modifier: Modifier = Modifier
) {
    val density = LocalDensity.current.density
    val scope = rememberCoroutineScope()
    var frame by remember { mutableStateOf(IntSize.Zero) }
    // 読み込むまでは 3:2 とみなす（PC・Web と同じ）。
    var natural by remember(path) { mutableStateOf<Pair<Float, Float>?>(null) }

    // 写真の動き。path ごとに作り直すので、次の写真は必ず静止した位置から出る。
    val x = remember(path) { Animatable(0f) }
    val y = remember(path) { Animatable(0f) }
    val rotation = remember(path) { Animatable(0f) }
    var busy by remember(path) { mutableStateOf(false) }
    var look by remember(path) { mutableStateOf(SlideshowGesture.Feedback(null, 0f, 0f)) }
    val busyNow by rememberUpdatedState(busy)
    val decideNow by rememberUpdatedState(onDecide)

    val widthDp = frame.width / density
    val heightDp = frame.height / density

    /** 決めて、写真を飛ばし、core へ渡す。 */
    fun decide(kind: SlideDecision) {
        if (busy) return
        busy = true
        look = SlideshowGesture.Feedback(kind, 1f, 0f)
        // pointerInput に捕まえられた古い値を使わないよう、枠の大きさはここで読む。
        val fly = SlideshowGesture.flyTarget(kind, frame.width / density, frame.height / density)
        scope.launch {
            coroutineScope {
                val spec = tween<Float>(FLY_MS, easing = FastOutSlowInEasing)
                launch { x.animateTo(fly.x * density, spec) }
                launch { y.animateTo(fly.y * density, spec) }
                launch { rotation.animateTo(fly.rotation, spec) }
            }
            decideNow(kind)
            // 写真が変わらなかったときのために戻す（変わったときは path ごと作り直される）。
            x.snapTo(0f); y.snapTo(0f); rotation.snapTo(0f)
            look = SlideshowGesture.Feedback(null, 0f, 0f)
            busy = false
        }
    }

    fun snapBack() {
        look = SlideshowGesture.Feedback(null, 0f, 0f)
        scope.launch {
            coroutineScope {
                val spec = tween<Float>(SNAP_MS, easing = FastOutSlowInEasing)
                launch { x.animateTo(0f, spec) }
                launch { y.animateTo(0f, spec) }
                launch { rotation.animateTo(0f, spec) }
            }
        }
    }

    val (boxW, boxH) = SlideshowGesture.fitContain(
        natural?.first ?: 3f, natural?.second ?: 2f,
        maxOf(0f, widthDp - MARGIN_DP * 2), maxOf(0f, heightDp - MARGIN_DP * 2)
    )

    Box(
        modifier
            .onSizeChanged { frame = it }
            .pointerInput(path) {
                awaitEachGesture {
                    // 子（拡大・★5・連写の印のボタン）が受け取った操作はここに来ない。
                    val down = awaitFirstDown()
                    if (busyNow) return@awaitEachGesture
                    val startX = down.position.x
                    val startY = down.position.y
                    var moved = false
                    val widthNow = size.width / density
                    while (true) {
                        val change = awaitPointerEvent().changes.firstOrNull { it.id == down.id }
                            ?: break
                        val dx = (change.position.x - startX) / density
                        val dy = (change.position.y - startY) / density
                        if (!change.pressed) {
                            if (!moved) {
                                // タップ。左半分は落とす、右半分は残す（長押しの拡大はしない）。
                                decide(
                                    SlideshowGesture.tapDecision(
                                        change.position.x, 0f, size.width.toFloat()
                                    )
                                )
                            } else {
                                val decision = SlideshowGesture.judgeDrag(dx, dy, widthNow)
                                if (decision != null) decide(decision) else snapBack()
                            }
                            break
                        }
                        if (!moved && SlideshowGesture.isTap(dx, dy)) continue
                        moved = true
                        change.consume()
                        val feedback = SlideshowGesture.dragFeedback(dx, dy, widthNow)
                        look = feedback
                        scope.launch(start = CoroutineStart.UNDISPATCHED) {
                            x.snapTo(dx * density)
                            y.snapTo(dy * density)
                            rotation.snapTo(feedback.rotation)
                        }
                    }
                }
            },
        contentAlignment = Alignment.Center
    ) {
        Box(
            Modifier
                .size(boxW.dp, boxH.dp)
                .graphicsLayer {
                    translationX = x.value
                    translationY = y.value
                    rotationZ = rotation.value
                }
                .clip(RoundedCornerShape(6.dp))
                .background(Tile)
                // 連写は白い縁で束ねた紙に見せる（PC・Web と同じ考え方）。
                .then(if (stands > 1) Modifier.border(3.dp, Color.White, RoundedCornerShape(6.dp)) else Modifier)
        ) {
            if (photo == null) {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    Text("読めません", fontSize = 11.sp, color = Faint)
                }
            } else {
                AsyncImage(
                    model = ImageRequest.Builder(LocalContext.current)
                        .data(photo.displayModel(displayEdge))
                        .size(1600)
                        .build(),
                    contentDescription = photo.name,
                    imageLoader = Images.loader(LocalContext.current),
                    contentScale = ContentScale.Fit,
                    onSuccess = { state ->
                        val size = state.painter.intrinsicSize
                        if (size.width > 0f && size.height > 0f) natural = size.width to size.height
                    },
                    modifier = Modifier.fillMaxSize()
                )
            }

            // ドラッグ中だけ、色の膜＋アイコン＋文字がドラッグ量に応じて濃くなる。
            val direction = look.direction
            if (direction != null) {
                val color = when (direction) {
                    SlideDecision.Drop -> DropColor
                    SlideDecision.Keep -> KeepColor
                    SlideDecision.Top -> Lime
                }
                val content = if (direction == SlideDecision.Top) Color.Black else Color.White
                Box(
                    Modifier.fillMaxSize().background(color.copy(alpha = 0.55f * look.strength)),
                    contentAlignment = Alignment.Center
                ) {
                    Column(
                        Modifier.graphicsLayer { alpha = look.strength },
                        horizontalAlignment = Alignment.CenterHorizontally
                    ) {
                        Icon(
                            when (direction) {
                                SlideDecision.Drop -> Icons.Filled.Close
                                SlideDecision.Keep -> Icons.Filled.Favorite
                                SlideDecision.Top -> Icons.Filled.Star
                            },
                            null, Modifier.size(64.dp), tint = content
                        )
                        Text(
                            when (direction) {
                                SlideDecision.Drop -> "落とす"
                                SlideDecision.Keep -> "残す"
                                SlideDecision.Top -> "★5 で確定"
                            },
                            fontSize = 28.sp, fontWeight = FontWeight.ExtraBold, color = content
                        )
                    }
                }
            }

            // トーナメントと同じ「連写 n 枚」の印と、右上の「拡大」「★5 で確定」。
            if (stands > 1) {
                BurstBadge(stands, onOpenBurst, Modifier.align(Alignment.TopStart).padding(6.dp))
            }
            if (photo != null) {
                TileTools(onZoom, { decide(SlideDecision.Top) }, Modifier.align(Alignment.TopEnd))
            }
        }
    }
}
