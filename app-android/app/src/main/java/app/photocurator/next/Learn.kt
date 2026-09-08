package app.photocurator.next

import android.content.Context
import android.util.Log
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Close
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
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import uniffi.photo_curator_core.BurstAnswer
import uniffi.photo_curator_core.PhotoRef
import uniffi.photo_curator_core.hashDistance
import uniffi.photo_curator_core.learnDistance
import java.io.File

/** 学習に出す 1 問。**隣どうしの 2 枚と、その距離。** */
data class Question(
    val left: PhotoRef,
    val right: PhotoRef,
    val distance: Int,
    val gapMs: Long
)

/**
 * 「同じ連写か」の基準を、その人の写真で決める。
 *
 * 既定値（距離 9）は私が Camera 694 枚を測って決めた値で、
 * **人によって撮り方が違えば境目も違う。** 数問だけ答えてもらって、
 * その人の写真での境目を出す。
 */
object Learning {
    private const val TAG = "Learning"
    private const val QUESTIONS = 8

    private fun file(context: Context, projectId: String) =
        File(context.filesDir, "threshold-$projectId.json")

    /** 学習ずみの距離。まだなら null。 */
    suspend fun learned(context: Context, projectId: String): Int? =
        withContext(Dispatchers.IO) {
            val target = file(context, projectId)
            if (!target.exists()) return@withContext null
            try {
                org.json.JSONObject(target.readText()).getInt("distance")
            } catch (error: Exception) {
                Log.w(TAG, "基準を読めなかった: $projectId", error)
                null
            }
        }

    suspend fun save(context: Context, projectId: String, distance: Int) =
        withContext(Dispatchers.IO) {
            try {
                file(context, projectId)
                    .writeText(org.json.JSONObject().put("distance", distance).toString())
                Unit
            } catch (error: Exception) {
                Log.w(TAG, "基準を保存できなかった: $projectId", error)
            }
        }

    /**
     * 問いを選ぶ。**境目のあたりを、広く薄く聞く。**
     *
     * 全部が「明らかに同じ」だったり「明らかに別」だったりすると、
     * どこが境目なのか分からない。距離順に並べて等間隔に拾うことで、
     * 近いものから遠いものまでひととおり見てもらう。
     */
    fun questions(refs: List<PhotoRef>, windowMs: Long): List<Question> {
        val candidates = ArrayList<Question>()
        for (at in 1 until refs.size) {
            val left = refs[at - 1]
            val right = refs[at]
            val leftHash = left.dHash ?: continue
            val rightHash = right.dHash ?: continue
            val leftAt = left.capturedAt ?: continue
            val rightAt = right.capturedAt ?: continue
            val gap = kotlin.math.abs(leftAt - rightAt)
            // 時間が離れているものは、そもそも連写ではない。聞くだけ無駄。
            if (gap > windowMs) continue
            val distance = hashDistance(leftHash, rightHash).toInt()
            // 0-2 は誰でも「同じ」、25 以上は誰でも「別」。**迷う帯だけ聞く。**
            if (distance < 3 || distance > 24) continue
            candidates += Question(left, right, distance, gap)
        }
        if (candidates.isEmpty()) return emptyList()

        val sorted = candidates.sortedBy { it.distance }
        if (sorted.size <= QUESTIONS) return sorted
        // 等間隔に拾う。
        return (0 until QUESTIONS).map { index ->
            sorted[index * (sorted.size - 1) / (QUESTIONS - 1)]
        }.distinct()
    }

    /** 答えから基準を出す。**判断は core が持つ。** */
    fun decide(answers: List<Pair<Int, Boolean>>, fallback: Int): Int =
        learnDistance(
            answers.map { BurstAnswer(distance = it.first.toUInt(), same = it.second) },
            fallback.toUInt()
        ).toInt()
}

/**
 * 「この 2 枚は同じ連写ですか？」を最大 8 問。
 *
 * 選別画面と同じ骨格。ただし**この画面にだけ下の帯**があり、
 * 答えは「別の写真」「同じ連写」の 2 つだけ。迷ったら飛ばせる。
 */
@Composable
fun LearnScreen(
    project: Project,
    questions: List<Question>,
    byPath: Map<String, Photo>,
    onDone: (List<Pair<Int, Boolean>>) -> Unit,
    onBack: () -> Unit
) {
    var at by remember { mutableStateOf(0) }
    var answers by remember { mutableStateOf<List<Pair<Int, Boolean>>>(emptyList()) }

    fun answerWith(same: Boolean) {
        val question = questions.getOrNull(at) ?: return
        answers = answers + (question.distance to same)
        if (at + 1 >= questions.size) onDone(answers + (question.distance to same))
        else at += 1
    }

    val question = questions.getOrNull(at)
    if (question == null) {
        // 問いが作れなかった。**黙って進まず、既定値で進むと言う。**
        LaunchedEffect(Unit) { onDone(emptyList()) }
        return
    }

    Column(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing)) {
        Row(
            Modifier.fillMaxWidth().height(56.dp).padding(end = 12.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            IconButton(onClick = onBack) { Icon(Icons.Filled.ArrowBack, "やめて戻る") }
            Text("この 2 枚は同じ連写ですか？", fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.weight(1f))
            Text(
                "${at + 1} / ${questions.size} 問 · 撮影間隔 " +
                    "${"%.1f".format(question.gapMs / 1000.0)} 秒",
                fontSize = 12.sp, color = Faint
            )
            Spacer(Modifier.width(12.dp))
            // **答えた分だけで決めて進める。** 8 問に付き合わせない。
            TextButton(onClick = { onDone(answers) }) {
                Text("残りをスキップ", fontSize = 12.sp)
            }
        }

        LinearProgressIndicator(
            progress = { (at + 1).toFloat() / questions.size },
            modifier = Modifier.fillMaxWidth().height(2.dp),
            color = Lime, trackColor = Color(0xFF24272D), gapSize = 0.dp, drawStopIndicator = {}
        )

        Row(Modifier.weight(1f).padding(6.dp)) {
            for (ref in listOf(question.left, question.right)) {
                Box(Modifier.weight(1f).fillMaxHeight().padding(3.dp)) {
                    QuestionTile(byPath[ref.relativePath], ref.capturedAt)
                }
            }
        }

        // ---- この画面だけの帯 ----
        Row(
            Modifier.fillMaxWidth().height(72.dp).padding(horizontal = 16.dp),
            horizontalArrangement = Arrangement.Center,
            verticalAlignment = Alignment.CenterVertically
        ) {
            OutlinedButton(
                onClick = { answerWith(false) },
                shape = RoundedCornerShape(50),
                modifier = Modifier.widthIn(max = 300.dp).weight(1f).height(48.dp)
            ) {
                Icon(Icons.Filled.Close, null, Modifier.size(18.dp))
                Spacer(Modifier.width(6.dp))
                Text("別の写真", fontWeight = FontWeight.Bold)
            }
            Spacer(Modifier.width(12.dp))
            Button(
                onClick = { answerWith(true) },
                shape = RoundedCornerShape(50),
                modifier = Modifier.widthIn(max = 300.dp).weight(1f).height(48.dp)
            ) {
                Icon(Icons.Filled.Check, null, Modifier.size(18.dp))
                Spacer(Modifier.width(6.dp))
                Text("同じ連写", fontWeight = FontWeight.Bold)
            }
        }
    }
}

@Composable
private fun QuestionTile(photo: Photo?, capturedAt: Long?) {
    Box(
        Modifier.fillMaxSize().clip(RoundedCornerShape(8.dp)).background(Tile)
    ) {
        if (photo == null) {
            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                Text("読めません", fontSize = 11.sp, color = Faint)
            }
        } else {
            AsyncImage(
                model = ImageRequest.Builder(LocalContext.current)
                    .data(photo.uri).size(1280).build(),
                contentDescription = photo.name,
                contentScale = ContentScale.Fit,
                modifier = Modifier.fillMaxSize()
            )
            // **どの 2 枚を見ているかが分かるように**、名前と時刻を添える。
            Column(
                Modifier
                    .padding(6.dp)
                    .clip(RoundedCornerShape(6.dp))
                    .background(Color(0xB3101114))
                    .padding(horizontal = 7.dp, vertical = 3.dp)
            ) {
                Text(photo.name, fontSize = 11.sp)
                capturedAt?.let {
                    Text(
                        java.text.SimpleDateFormat("HH:mm:ss", java.util.Locale.JAPAN)
                            .format(java.util.Date(it)),
                        fontSize = 10.sp, color = Faint
                    )
                }
            }
        }
    }
}

/**
 * 学習で決めた基準を作る 1 か所。
 *
 * 既定の 9 は Camera 694 枚の実測（0-9 に連写 28 組、15 以上に別の絵 130 組）
 * から決めた値。**学習していない人にも使える出発点**として置いてある。
 */
const val DEFAULT_DISTANCE = 9

fun thresholdFor(distance: Int): uniffi.photo_curator_core.BurstThreshold =
    uniffi.photo_curator_core.BurstThreshold(
        windowMs = 4000,
        distance = distance.coerceIn(2, 24).toUInt(),
        dHashVersion = Analyse.VERSION
    )
