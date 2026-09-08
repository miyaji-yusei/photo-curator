package app.photocurator.next

import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.launch
import uniffi.photo_curator_core.PhotoRef

/**
 * 学習 → 確認 → 開始 の道。
 *
 * 連写をまとめる設定で、**その プロジェクトで初めて始めるときだけ**通る。
 * 2 回目からは決めた基準をそのまま使う（設定の `…` から学び直せる）。
 */
@Composable
fun LearnFlow(project: Project, onStart: () -> Unit, onBack: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()

    var photos by remember { mutableStateOf<List<Photo>>(emptyList()) }
    var refs by remember { mutableStateOf<List<PhotoRef>>(emptyList()) }
    var questions by remember { mutableStateOf<List<Question>>(emptyList()) }
    var prepared by remember { mutableStateOf(0 to 0) }
    var phase by remember { mutableStateOf("prepare") }
    var distance by remember { mutableStateOf(DEFAULT_DISTANCE) }

    val byPath = remember(photos) { photos.associateBy { it.relativePath } }

    LaunchedEffect(project.id) {
        val ready = Prepare.run(context, project) { done, total -> prepared = done to total }
        photos = ready.first
        refs = ready.second
        questions = Learning.questions(refs, 4000)
        // 迷う組が無いアルバムもある。**問いが作れないなら聞かない。**
        phase = if (questions.isEmpty()) "preview" else "learn"
    }

    when (phase) {
        "prepare" -> Box(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing)) {
            IconButton(onClick = onBack, modifier = Modifier.align(Alignment.TopStart)) {
                Icon(Icons.Filled.ArrowBack, "やめて戻る")
            }
            Column(
                Modifier.align(Alignment.Center),
                horizontalAlignment = Alignment.CenterHorizontally
            ) {
                Text("似た写真を調べています…", color = Faint, fontSize = 13.sp)
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

        "learn" -> LearnScreen(
            project = project,
            questions = questions,
            byPath = byPath,
            onDone = { answers ->
                // 答えが 0 問でも進む。**既定値で進めると決めてある。**
                distance = Learning.decide(answers, DEFAULT_DISTANCE)
                phase = "preview"
            },
            onBack = onBack
        )

        else -> PreviewScreen(
            project = project,
            refs = refs,
            byPath = byPath,
            learned = distance,
            groupSize = Prefs.groupSize(context),
            onStart = { chosen ->
                // **ここで初めて確定する。** スライダーを動かしただけでは決まらない。
                scope.launch {
                    Learning.save(context, project.id, chosen)
                    onStart()
                }
            },
            onBack = onBack
        )
    }
}
