//! Photo Curator の中核。**判断が絡むものだけを置く。**
//!
//! バイトを読む・画像をデコードする・画面に出すのは各環境の仕事。
//! ここに置くのは「同じ入力なら同じ答えでなければ困るもの」だけ。
//!
//! いまは連写のまとめ方が Rust（PC）と TypeScript（Web）に二重実装されていて、
//! 片方だけ直る不具合が実際に起きた。作り直しでここを 1 つに畳む。

uniffi::include_scaffolding!("photo_curator_core");

#[derive(Debug, Clone)]
pub struct PhotoRef {
    pub relative_path: String,
    pub captured_at: Option<i64>,
    pub d_hash: Option<String>,
    pub d_hash_version: i32,
}

#[derive(Debug, Clone)]
pub struct BurstPair {
    pub left: PhotoRef,
    pub right: PhotoRef,
}

#[derive(Debug, Clone)]
pub struct BurstThreshold {
    pub window_ms: i64,
    pub distance: u32,
    pub d_hash_version: i32,
}

#[derive(Debug, Clone)]
pub struct PairOverride {
    pub left: String,
    pub right: String,
    pub decision: String,
}

#[derive(Debug, Clone)]
pub struct BurstGroup {
    pub members: Vec<String>,
    pub representative: String,
}

/// dHash どうしの距離。**0 が同一。**
///
/// 16 進 16 桁を 64bit として読み、立っているビットの差を数える。
/// 読めない文字列が来たら「まったく違う」（64）として扱う。
/// 0 を返すと**読めない写真どうしが同一に見えて**、誤ってまとまる。
pub fn hash_distance(left: String, right: String) -> u32 {
    match (
        u64::from_str_radix(&left, 16),
        u64::from_str_radix(&right, 16),
    ) {
        (Ok(a), Ok(b)) => (a ^ b).count_ones(),
        _ => 64,
    }
}

/// 2 枚が同じ連写か。**時間と見た目の両方**が近いときだけ真。
///
/// 時間だけで見ると、同じ場所で 1 時間後に撮った別の写真が混ざる。
/// 見た目だけで見ると、日をまたいだ同じ構図が混ざる。
pub fn is_same_burst(pair: BurstPair, threshold: BurstThreshold) -> bool {
    let (Some(left_at), Some(right_at)) = (pair.left.captured_at, pair.right.captured_at) else {
        // 撮影時刻が読めないものは、まとめない。**推測でまとめると戻せない。**
        return false;
    };
    if (left_at - right_at).abs() > threshold.window_ms {
        return false;
    }
    let (Some(left_hash), Some(right_hash)) = (&pair.left.d_hash, &pair.right.d_hash) else {
        return false;
    };
    // 版が違う dHash は比べない。作り方が変わると値の意味が変わる。
    if pair.left.d_hash_version != threshold.d_hash_version
        || pair.right.d_hash_version != threshold.d_hash_version
    {
        return false;
    }
    hash_distance(left_hash.clone(), right_hash.clone()) <= threshold.distance
}

/// 撮影順に並んだ写真を、連写のまとまりへ畳む。
///
/// **隣どうしだけを見る。** 連続するペアがすべて基準を満たす区間が 1 つの
/// まとまりになる。飛び石でまとめると、どこで切れているか説明できなくなる。
///
/// 人が手で直した `overrides` は基準より優先する。
pub fn group_bursts(
    photos: Vec<PhotoRef>,
    threshold: BurstThreshold,
    overrides: Vec<PairOverride>,
) -> Vec<BurstGroup> {
    let mut groups: Vec<BurstGroup> = Vec::new();
    let mut current: Vec<String> = Vec::new();

    let decide = |left: &PhotoRef, right: &PhotoRef| -> bool {
        // 手で直したものが最優先。**基準を動かしても人の判断は残る。**
        for over in &overrides {
            if over.left == left.relative_path && over.right == right.relative_path {
                return over.decision == "join";
            }
        }
        is_same_burst(
            BurstPair {
                left: left.clone(),
                right: right.clone(),
            },
            threshold.clone(),
        )
    };

    for (index, photo) in photos.iter().enumerate() {
        if current.is_empty() {
            current.push(photo.relative_path.clone());
            continue;
        }
        let previous = &photos[index - 1];
        if decide(previous, photo) {
            current.push(photo.relative_path.clone());
        } else {
            groups.push(finish(&mut current));
            current.push(photo.relative_path.clone());
        }
    }
    if !current.is_empty() {
        groups.push(finish(&mut current));
    }
    groups
}

/// たまった分を 1 つのまとまりにして、器を空にする。
fn finish(current: &mut Vec<String>) -> BurstGroup {
    let members = std::mem::take(current);
    let representative = members.first().cloned().unwrap_or_default();
    BurstGroup {
        members,
        representative,
    }
}


// ---------------------------------------------------------------------------
// dHash
// ---------------------------------------------------------------------------

/// dHash を作るときの一辺。9x8 の輝度から横の差分 64 個を取る。
pub const D_HASH_WIDTH: u32 = 9;
pub const D_HASH_HEIGHT: u32 = 8;
/// いまの作り方の版。**変えたら上げる。** 版が違う値は比べない。
pub const D_HASH_VERSION: i32 = 2;

/// 9x8 の輝度から dHash を作る。
///
/// **デコードと縮小は各環境に任せる。** Android は BitmapFactory、
/// Web は createImageBitmap が一番速い。ここに置くのは「同じ画素なら
/// 同じ値でなければ困る」部分だけ。
///
/// `luma` は 9x8 = 72 個の輝度（0-255）を、左上から行優先で並べたもの。
/// 長さが違えば None。**黙って 0 を返すと、読めない写真が全部同じ値になり、
/// 誤ってまとまる。**
pub fn d_hash_from_luma(luma: Vec<u8>) -> Option<String> {
    let expected = (D_HASH_WIDTH * D_HASH_HEIGHT) as usize;
    if luma.len() != expected {
        return None;
    }
    let mut bits: u64 = 0;
    let mut at = 0;
    for row in 0..D_HASH_HEIGHT as usize {
        for col in 0..(D_HASH_WIDTH as usize - 1) {
            let left = luma[row * D_HASH_WIDTH as usize + col];
            let right = luma[row * D_HASH_WIDTH as usize + col + 1];
            // 左が右より明るければ 1。**絶対値ではなく隣との差**を見るので、
            // 全体の明るさが変わっても値が動かない。
            if left > right {
                bits |= 1 << at;
            }
            at += 1;
        }
    }
    Some(format!("{bits:016x}"))
}

/// 任意の大きさの輝度画像から dHash を作る。**縮小もここでやる。**
///
/// `d_hash_from_luma` は 9x8 に潰し終えたものを受け取るが、潰し方を各環境に
/// 任せると値が揃わない。Android の `createScaledBitmap` は縮小率が大きいと
/// 2x2 しか読まないので、**同じ絵を JPEG で作り直しただけで距離が 5 開いた**。
/// 閾値 6 のすぐ隣で、これでは連写かどうかを判断できない。
///
/// ここでは升目の平均を取る。全画素を読むので、縮小率が大きくても値が飛ばない。
/// 各環境は「輝度を並べて渡す」だけになり、**答えは 1 か所で決まる。**
pub fn d_hash_from_gray(gray: Vec<u8>, width: u32, height: u32) -> Option<String> {
    if width < D_HASH_WIDTH || height < D_HASH_HEIGHT {
        return None;
    }
    if gray.len() != (width as usize) * (height as usize) {
        return None;
    }
    let mut cells = vec![0u8; (D_HASH_WIDTH * D_HASH_HEIGHT) as usize];
    for row in 0..D_HASH_HEIGHT {
        // 端数は上下の升に散らす。切り捨てだけだと右端と下端が痩せる。
        let top = (row * height / D_HASH_HEIGHT) as usize;
        let bottom = ((row + 1) * height / D_HASH_HEIGHT) as usize;
        for col in 0..D_HASH_WIDTH {
            let left = (col * width / D_HASH_WIDTH) as usize;
            let right = ((col + 1) * width / D_HASH_WIDTH) as usize;
            let mut total: u64 = 0;
            let mut count: u64 = 0;
            for y in top..bottom {
                for x in left..right {
                    total += gray[y * width as usize + x] as u64;
                    count += 1;
                }
            }
            // width >= 9 かつ height >= 8 なので count が 0 になることはない。
            cells[(row * D_HASH_WIDTH + col) as usize] = (total / count.max(1)) as u8;
        }
    }
    d_hash_from_luma(cells)
}

/// 学習の 1 問への答え。**距離と、人の判断だけ。**
#[derive(Debug, Clone)]
pub struct BurstAnswer {
    pub distance: u32,
    pub same: bool,
}

/// 答えから「見た目が近い」の境目を決める。
///
/// **人が「同じ」と言った一番遠いところと、「別」と言った一番近いところの
/// あいだに置く。** 答えが 1 つも無ければ既定値のまま。
///
/// 答えが食い違ったとき（12 を同じ、8 を別、のような）は**切る側に倒す**。
/// 間違えて繋ぐと片方が二度と画面に出ず、選んだ覚えのない星が付く。
/// 切りすぎたときは両方見えるだけで、気付けるし直せる。
pub fn learn_distance(answers: Vec<BurstAnswer>, fallback: u32) -> u32 {
    let same_max = answers
        .iter()
        .filter(|answer| answer.same)
        .map(|answer| answer.distance)
        .max();
    let different_min = answers
        .iter()
        .filter(|answer| !answer.same)
        .map(|answer| answer.distance)
        .min();

    let learned = match (same_max, different_min) {
        (None, None) => fallback,
        // 「同じ」しか無い。少なくともそこまでは繋ぐ。
        (Some(same), None) => same.max(fallback),
        // 「別」しか無い。そのすぐ手前で切る。
        (None, Some(different)) => different.saturating_sub(1),
        (Some(same), Some(different)) => {
            if same < different {
                // 素直に境目がある。真ん中に置く。
                same + (different - same) / 2
            } else {
                // 食い違っている。**切る側に倒す。**
                different.saturating_sub(1)
            }
        }
    };
    // 極端な値は基準として使えない。0 は何も繋がらず、大きすぎると何でも繋がる。
    learned.clamp(2, 24)
}

// ---------------------------------------------------------------------------
// 選別
// ---------------------------------------------------------------------------

use std::collections::HashMap;

/// 星の上限。これに達した写真は以降のラウンドに出ない。
const MAX_STAR: i32 = 5;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Decision {
    pub group: Vec<String>,
    pub chosen: Vec<String>,
    /// ★5 で確定したなら、その 1 枚と**その前の星**。
    /// 戻すときは上げ下げではなく元の星に返す必要がある。
    #[serde(default)]
    pub topped: Option<Topped>,
}

/// ★5 で確定した 1 枚の控え。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Topped {
    pub path: String,
    pub previous: i32,
}

/// 選別の途中。**丸ごと保存して、丸ごと読み戻す。**
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Session {
    pub group_size: u32,
    pub target_star: i32,
    pub round: u32,
    pub queue: Vec<String>,
    pub current: Vec<String>,
    pub survivors: Vec<String>,
    pub ratings: HashMap<String, i32>,
    pub members: HashMap<String, Vec<String>>,
    pub history: Vec<Decision>,
    pub finished: bool,
}

/// ラウンドを始める。
///
/// 連写をまとめる場合、**画面に出るのは代表だけ**。仲間は members に控え、
/// 代表が通れば同じ星を配る。畳まないと、同じ構図を何度も見ることになる。
pub fn start_round(
    photos: Vec<PhotoRef>,
    group_size: u32,
    target_star: i32,
    group_bursts_on: bool,
    threshold: BurstThreshold,
    overrides: Vec<PairOverride>,
) -> Session {
    let mut ratings = HashMap::new();
    for photo in &photos {
        ratings.insert(photo.relative_path.clone(), target_star);
    }

    let (queue, members) = if group_bursts_on {
        let groups = group_bursts(photos, threshold, overrides);
        let mut members = HashMap::new();
        let mut queue = Vec::with_capacity(groups.len());
        for group in groups {
            queue.push(group.representative.clone());
            // 単独の写真は控えない。**持たないものは食い違わない。**
            if group.members.len() > 1 {
                members.insert(group.representative.clone(), group.members);
            }
        }
        (queue, members)
    } else {
        (
            photos.iter().map(|p| p.relative_path.clone()).collect(),
            HashMap::new(),
        )
    };

    let mut session = Session {
        group_size: group_size.max(2),
        target_star,
        round: 1,
        queue,
        current: Vec::new(),
        survivors: Vec::new(),
        ratings,
        members,
        history: Vec::new(),
        finished: false,
    };
    fill(&mut session);
    session
}

/// 次に見せる分を queue から取り出す。取り出せなければ終わり。
fn fill(session: &mut Session) {
    let take = (session.group_size as usize).min(session.queue.len());
    session.current = session.queue.drain(..take).collect();
    session.finished = session.current.is_empty();
}

/// 星を配る。まとまりの仲間にも同じだけ動かす。
fn shift_star(session: &mut Session, id: &str, delta: i32) {
    let star = session.ratings.get(id).copied().unwrap_or(0);
    session
        .ratings
        .insert(id.to_string(), (star + delta).clamp(0, MAX_STAR));
    let Some(mates) = session.members.get(id).cloned() else {
        return;
    };
    for mate in mates {
        if mate == id {
            continue;
        }
        let mate_star = session.ratings.get(&mate).copied().unwrap_or(0);
        session
            .ratings
            .insert(mate, (mate_star + delta).clamp(0, MAX_STAR));
    }
}

/// いまのグループを確定して次へ。
///
/// **選ばれたものだけ星が 1 つ上がる。** 選ばれなかったものは据え置きで、
/// そのラウンドから外れる。「落とす」は星を下げることではない。
pub fn advance(session: Session, selected: Vec<String>) -> Session {
    let mut next = session;
    let group = next.current.clone();
    // 画面に無いものが渡ってきても無視する。**呼び出し側を信用しない。**
    let chosen: Vec<String> = selected
        .into_iter()
        .filter(|id| group.contains(id))
        .collect();

    for id in &chosen {
        next.survivors.push(id.clone());
        // 仲間には星だけ配り、survivors には入れない。
        // 入れると次のラウンドで仲間が全部出てきて、畳んだ意味が消える。
        shift_star(&mut next, id, 1);
    }

    next.history.push(Decision { group, chosen, topped: None });
    fill(&mut next);
    next
}

/// 直前の判断を取り消す。**星も戻す。**
///
/// 戻せるのは「進んだこと」と「上げた星」の両方。星だけ残ると、
/// 次のラウンドに出てくる顔ぶれが変わってしまう。
pub fn undo(session: Session) -> Session {
    let mut next = session;
    let Some(last) = next.history.pop() else {
        return next;
    };

    // いま出している分を queue の先頭へ戻す。
    let mut queue = last.group.clone();
    queue.append(&mut next.current);
    queue.append(&mut next.queue);
    next.queue = queue;

    for id in &last.chosen {
        if let Some(position) = next.survivors.iter().rposition(|s| s == id) {
            next.survivors.remove(position);
        }
        shift_star(&mut next, id, -1);
    }

    // ★5 で確定した分は 1 つ下げるのではなく、**押す前の星に返す**。
    // 5 から 1 つ下げると 4 が残り、押していないはずの星が残る。
    if let Some(top) = &last.topped {
        for id in family(&next, &top.path) {
            next.ratings.insert(id, top.previous);
        }
    }

    fill(&mut next);
    next
}

/// その 1 枚と、まとまりの仲間。**星は代表と仲間で必ず揃える。**
fn family(session: &Session, path: &str) -> Vec<String> {
    let mut all = session.members.get(path).cloned().unwrap_or_default();
    if !all.iter().any(|id| id == path) {
        all.push(path.to_string());
    }
    all
}

/// ★5 を付けてそのグループを確定する。**以降のラウンドには出さない。**
///
/// 「これは決まり」と分かっている 1 枚を、ラウンドを重ねて 5 回選ばせるのは
/// ただの作業になる。星を最高にして survivors から外すので、次のラウンドの
/// 顔ぶれには出てこない。**戻せる**（1 つ戻すで元の星に返る）。
///
/// 画面に出ていない写真を指されたら何もしない。呼び出し側を信用しない。
pub fn keep_top(session: Session, path: String) -> Session {
    if !session.current.iter().any(|id| id == &path) {
        return session;
    }
    let previous = session.ratings.get(&path).copied().unwrap_or(0);
    let mut next = advance(session, vec![path.clone()]);
    for id in family(&next, &path) {
        next.ratings.insert(id, MAX_STAR);
    }
    if let Some(position) = next.survivors.iter().rposition(|s| s == &path) {
        next.survivors.remove(position);
    }
    if let Some(last) = next.history.last_mut() {
        last.topped = Some(Topped { path, previous });
    }
    next
}


/// 連写のまとまりで、画面に出す 1 枚を選び直す。
///
/// 既定の代表は撮影順の先頭だが、**先頭がぶれていることは普通にある。**
/// 代表 1 枚しか見えないと、その 1 枚の出来でまとまり全体の運命が決まる。
/// 仲間を見て選び直せなければ、畳んだことがそのまま取りこぼしになる。
///
/// **星は動かさない。** これは「どれを見せるか」の話であって、
/// 「どれを残すか」ではない。残すかどうかは今までどおり advance が決める。
///
/// 選び直せないときは None:
/// - まとまりでない（仲間がいない）
/// - 仲間でないものを指した
/// - もう見終わった（history にある）まとまり。**済んだ判断は動かさない。**
pub fn set_representative(session: Session, shown: String, wanted: String) -> Option<Session> {
    let mates = session.members.get(&shown)?.clone();
    if !mates.contains(&wanted) {
        return None;
    }
    if shown == wanted {
        return Some(session);
    }
    // まだ画面に出ていないか、いま出ているものだけ。
    let in_current = session.current.iter().any(|id| id == &shown);
    let in_queue = session.queue.iter().any(|id| id == &shown);
    if !in_current && !in_queue {
        return None;
    }

    let mut next = session;
    for id in next.current.iter_mut().chain(next.queue.iter_mut()) {
        if *id == shown {
            *id = wanted.clone();
        }
    }
    // 仲間の顔ぶれは変えない。鍵だけ差し替える。
    next.members.remove(&shown);
    next.members.insert(wanted, mates);
    Some(next)
}

/// 一度に見比べる枚数を変える。**いまのグループにすぐ効く。**
///
/// 足りなければ次のグループから補い、余れば次のグループへ押し出す。
/// 見終わった分（history）は動かさない。選別の途中で「4 枚では多い」と
/// 気づいたときに、いったんやめて設定に戻る必要をなくすため。
pub fn resize(session: Session, group_size: u32) -> Session {
    let mut next = session;
    // いま出している分と、まだ見ていない分を 1 本に戻してから取り直す。
    let mut line = std::mem::take(&mut next.current);
    line.append(&mut next.queue);
    next.group_size = group_size.max(2);
    next.queue = line;
    fill(&mut next);
    next
}

/// まとめ方を変えて、いまのラウンドを組み直す。
///
/// **その場で組み直し、進んだ分は星として残す。** 手で直したのに次のラウンド
/// まで何も変わらないのでは、「押したのに何も起きない」と同じことになる。
///
/// 組み直すのは**まだ判断していない写真だけ**。決めた写真は決めたまま置く。
/// これで「どの写真も、決まっているか、これから出るか、どちらか一方」が保たれる。
/// 全部を並べ直すと、決めた写真と未決の写真が同じまとまりに入ってしまい、
/// 片方をもう一度見せるか、片方を黙って捨てるかしか選べなくなる。
///
/// 決めたまとまりの仲間は控えたまま残す。**戻すが効き続けるように。**
/// 代表を手で選び直していたときは、顔ぶれが変わっていなければそのまま使う。
pub fn regroup(
    session: Session,
    photos: Vec<PhotoRef>,
    group_bursts_on: bool,
    threshold: BurstThreshold,
    overrides: Vec<PairOverride>,
) -> Session {
    let mut next = session;

    // 決めた写真。history に出た代表の仲間まで含める。
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut kept_members: HashMap<String, Vec<String>> = HashMap::new();
    for decision in &next.history {
        for rep in &decision.group {
            match next.members.get(rep) {
                Some(mates) => {
                    for mate in mates {
                        seen.insert(mate.clone());
                    }
                    kept_members.insert(rep.clone(), mates.clone());
                }
                None => {
                    seen.insert(rep.clone());
                }
            }
        }
    }

    // 手で選んだ代表を覚えておく。顔ぶれが同じなら引き継ぐ。
    let chosen_reps: HashMap<Vec<String>, String> = next
        .members
        .iter()
        .map(|(rep, mates)| (mates.clone(), rep.clone()))
        .collect();

    let remaining: Vec<PhotoRef> = photos
        .into_iter()
        .filter(|photo| !seen.contains(&photo.relative_path))
        .collect();

    let (queue, mut members) = if group_bursts_on {
        let groups = group_bursts(remaining, threshold, overrides);
        let mut members = HashMap::new();
        let mut queue = Vec::with_capacity(groups.len());
        for group in groups {
            // 顔ぶれが変わっていなければ、人が選んだ代表を尊重する。
            let representative = chosen_reps
                .get(&group.members)
                .cloned()
                .unwrap_or(group.representative);
            queue.push(representative.clone());
            if group.members.len() > 1 {
                members.insert(representative, group.members);
            }
        }
        (queue, members)
    } else {
        (
            remaining
                .iter()
                .map(|photo| photo.relative_path.clone())
                .collect(),
            HashMap::new(),
        )
    };

    members.extend(kept_members);
    next.queue = queue;
    next.members = members;
    next.current = Vec::new();
    fill(&mut next);
    next
}

/// 次のラウンドへ。**前を通ったものだけが、星を持ったまま上がる。**
///
/// 進めないときは None を返す。「まだ続けられます」と言っておいて
/// 何も出ない画面を見せるより、進めないと先に言う方がよい。
/// 進めないのは 2 つ:
/// - 通ったものが 2 枚未満（比べる相手がいない）
/// - 星が上限（これ以上つけられない）
pub fn next_round(
    previous: Session,
    photos: Vec<PhotoRef>,
    group_bursts_on: bool,
    threshold: BurstThreshold,
    overrides: Vec<PairOverride>,
) -> Option<Session> {
    let target_star = previous.target_star + 1;
    if target_star >= MAX_STAR {
        return None;
    }
    if previous.survivors.len() < 2 {
        return None;
    }

    // survivors の並びではなく、渡された写真の並び（撮影順）を保つ。
    // 連写は隣どうしで畳むので、順が崩れるとまとまらなくなる。
    let remaining: Vec<PhotoRef> = photos
        .into_iter()
        .filter(|photo| previous.survivors.contains(&photo.relative_path))
        .collect();
    if remaining.len() < 2 {
        return None;
    }

    let mut session = start_round(
        remaining,
        previous.group_size,
        target_star,
        group_bursts_on,
        threshold,
        overrides,
    );
    session.round = previous.round + 1;
    // 星は引き継ぐ。start_round は target_star で埋め直すが、それでは
    // **畳まれて画面に出なかった仲間の星**と、落ちたものの星が消える。
    session.ratings = previous.ratings;
    // history は引き継がない。**戻すはラウンドをまたがない。**
    // またぐと、戻した先の round と target_star が合わなくなる。
    Some(session)
}

// ---------------------------------------------------------------------------
// 保存
// ---------------------------------------------------------------------------

/// 選別の途中を文字列にする。**形は core が持つ。**
///
/// 各環境が独自に組み立てると、端末をまたいだときに読めない。
/// サイドカー（NAS で共有する catalog.json）もこの形をそのまま入れる。
pub fn session_to_json(session: Session) -> String {
    serde_json::to_string(&session).unwrap_or_else(|_| "{}".into())
}

/// 読み戻す。**形が違えば null を返す。**
/// 中途半端に読むより、読まずに最初からやり直す方が安全。
pub fn session_from_json(json: String) -> Option<Session> {
    serde_json::from_str(&json).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn photo(name: &str, at: i64, hash: &str) -> PhotoRef {
        PhotoRef {
            relative_path: name.into(),
            captured_at: Some(at),
            d_hash: Some(hash.into()),
            d_hash_version: 2,
        }
    }

    fn threshold() -> BurstThreshold {
        BurstThreshold {
            window_ms: 4000,
            distance: 6,
            d_hash_version: 2,
        }
    }


    fn plain(names: &[&str]) -> Vec<PhotoRef> {
        names
            .iter()
            .enumerate()
            .map(|(index, name)| PhotoRef {
                relative_path: (*name).into(),
                // まとめが起きないよう、わざと離す。
                captured_at: Some(index as i64 * 100_000),
                d_hash: Some("0000000000000000".into()),
                d_hash_version: 2,
            })
            .collect()
    }

    fn round(names: &[&str], size: u32) -> Session {
        start_round(plain(names), size, 0, false, threshold(), vec![])
    }

    #[test]
    fn 星5で確定すると次のラウンドに出ない() {
        let session = round(&["1", "2", "3", "4"], 2);
        let after = keep_top(session, "1".into());
        assert_eq!(after.ratings.get("1"), Some(&5));
        // **survivors に残らない。** 残ると次のラウンドで見比べる相手にされる。
        assert!(!after.survivors.contains(&"1".to_string()));
        // グループは進んでいる。
        assert_eq!(after.current, vec!["3", "4"]);
    }

    #[test]
    fn 星5を戻すと元の星に返る() {
        let session = round(&["1", "2", "3", "4"], 2);
        let topped = keep_top(session, "1".into());
        let back = undo(topped);
        // 5 から 1 つ下げた 4 ではなく、押す前の 0 に返る。
        assert_eq!(back.ratings.get("1").copied().unwrap_or(0), 0);
        assert_eq!(back.current, vec!["1", "2"]);
    }

    #[test]
    fn 画面に出ていない写真は星5にできない() {
        let session = round(&["1", "2", "3", "4"], 2);
        let after = keep_top(session.clone(), "4".into());
        assert_eq!(after.current, session.current);
        assert!(after.history.is_empty());
    }

    #[test]
    fn 始めると最初のグループが出ている() {
        let session = round(&["1", "2", "3", "4", "5"], 2);
        assert_eq!(session.current, vec!["1", "2"]);
        assert_eq!(session.queue, vec!["3", "4", "5"]);
        assert!(!session.finished);
    }

    #[test]
    fn 選ばれたものだけ星が上がる() {
        let session = round(&["1", "2", "3", "4"], 2);
        let after = advance(session, vec!["1".into()]);
        assert_eq!(after.ratings["1"], 1);
        // **落ちたものは下がらない。** 据え置きでラウンドから外れるだけ。
        assert_eq!(after.ratings["2"], 0);
        assert_eq!(after.survivors, vec!["1"]);
        assert_eq!(after.current, vec!["3", "4"]);
    }

    #[test]
    fn 一枚も選ばなくても進める() {
        // 良い写真が 1 枚も無いグループはある。
        let session = round(&["1", "2", "3", "4"], 2);
        let after = advance(session, vec![]);
        assert!(after.survivors.is_empty());
        assert_eq!(after.current, vec!["3", "4"]);
    }

    #[test]
    fn 画面に無いものを渡しても無視する() {
        let session = round(&["1", "2", "3", "4"], 2);
        let after = advance(session, vec!["3".into()]);
        assert!(after.survivors.is_empty(), "見えていない写真は通さない");
    }

    #[test]
    fn 最後まで進むと終わる() {
        let session = round(&["1", "2"], 2);
        let after = advance(session, vec!["1".into()]);
        assert!(after.finished);
        assert!(after.current.is_empty());
    }

    #[test]
    fn 一つ戻すと星も戻る() {
        let session = round(&["1", "2", "3", "4"], 2);
        let after = advance(session, vec!["1".into()]);
        let back = undo(after);
        assert_eq!(back.ratings["1"], 0, "星が戻らないと次の顔ぶれが変わる");
        assert!(back.survivors.is_empty());
        assert_eq!(back.current, vec!["1", "2"], "同じグループに戻る");
        assert_eq!(back.queue, vec!["3", "4"]);
    }

    #[test]
    fn 履歴が無ければ戻しても壊れない() {
        let session = round(&["1", "2"], 2);
        let back = undo(session.clone());
        assert_eq!(back.current, session.current);
    }

    #[test]
    fn まとめた仲間にも星が配られる() {
        // 時間も見た目も近い 3 枚 ＋ 離れた 1 枚。
        let photos = vec![
            photo("a1.jpg", 1000, "0000000000000000"),
            photo("a2.jpg", 2000, "0000000000000001"),
            photo("a3.jpg", 3000, "0000000000000003"),
            photo("b1.jpg", 500_000, "ffffffffffffffff"),
        ];
        let session = start_round(photos, 2, 0, true, threshold(), vec![]);
        // 画面に出るのは代表だけ。
        assert_eq!(session.current, vec!["a1.jpg", "b1.jpg"]);

        let after = advance(session, vec!["a1.jpg".into()]);
        // **仲間にも同じ星が付く。**
        assert_eq!(after.ratings["a1.jpg"], 1);
        assert_eq!(after.ratings["a2.jpg"], 1);
        assert_eq!(after.ratings["a3.jpg"], 1);
        // **仲間は survivors に入れない。** 入れると次のラウンドで全員出てくる。
        assert_eq!(after.survivors, vec!["a1.jpg"]);
    }

    #[test]
    fn まとめを戻すと仲間の星も戻る() {
        let photos = vec![
            photo("a1.jpg", 1000, "0000000000000000"),
            photo("a2.jpg", 2000, "0000000000000001"),
            photo("b1.jpg", 500_000, "ffffffffffffffff"),
        ];
        let session = start_round(photos, 2, 0, true, threshold(), vec![]);
        let after = advance(session, vec!["a1.jpg".into()]);
        let back = undo(after);
        assert_eq!(back.ratings["a1.jpg"], 0);
        assert_eq!(back.ratings["a2.jpg"], 0, "仲間だけ星が残ると辻褄が合わない");
    }


    #[test]
    fn 保存して読み戻すと同じ状態になる() {
        let session = round(&["1", "2", "3", "4"], 2);
        let after = advance(session, vec!["1".into()]);
        let json = session_to_json(after.clone());
        let back = session_from_json(json).expect("読み戻せる");
        assert_eq!(back.current, after.current);
        assert_eq!(back.queue, after.queue);
        assert_eq!(back.survivors, after.survivors);
        assert_eq!(back.ratings["1"], 1);
        assert_eq!(back.history.len(), 1);
    }

    #[test]
    fn 形が違う保存は読まない() {
        // **中途半端に読むより、読まずに最初からやり直す方が安全。**
        assert!(session_from_json("{}".into()).is_none());
        assert!(session_from_json("こわれている".into()).is_none());
        assert!(session_from_json(r#"{"groupSize":4}"#.into()).is_none());
    }

    #[test]
    fn 読み戻したものから選別を続けられる() {
        let session = round(&["1", "2", "3", "4", "5", "6"], 2);
        let after = advance(session, vec!["1".into()]);
        let back = session_from_json(session_to_json(after)).expect("読み戻せる");
        // **保存を挟んでも、戻すところまで含めて同じように動く。**
        let next = advance(back, vec!["3".into()]);
        assert_eq!(next.survivors, vec!["1", "3"]);
        let undone = undo(next);
        assert_eq!(undone.survivors, vec!["1"]);
        assert_eq!(undone.current, vec!["3", "4"]);
    }

    #[test]
    fn 星は上限で頭打ちになる() {
        let mut photos = plain(&["1", "2"]);
        photos[0].relative_path = "1".into();
        let mut session = start_round(photos, 2, 5, false, threshold(), vec![]);
        session.ratings.insert("1".into(), 5);
        let after = advance(session, vec!["1".into()]);
        assert_eq!(after.ratings["1"], 5);
    }


    /// 9x8 の輝度を作る補助。`f(row, col)` が輝度を返す。
    fn luma(f: impl Fn(usize, usize) -> u8) -> Vec<u8> {
        let mut out = Vec::with_capacity(72);
        for row in 0..8 {
            for col in 0..9 {
                out.push(f(row, col));
            }
        }
        out
    }

    #[test]
    fn 画素数が合わなければ作らない() {
        // **黙って 0 を返すと、読めない写真が全部同じ値になり誤ってまとまる。**
        assert!(d_hash_from_luma(vec![]).is_none());
        assert!(d_hash_from_luma(vec![0; 71]).is_none());
        assert!(d_hash_from_luma(vec![0; 73]).is_none());
    }

    #[test]
    fn 一様な画像は零になる() {
        // 隣と差が無ければビットは立たない。
        assert_eq!(d_hash_from_luma(luma(|_, _| 128)).unwrap(), "0000000000000000");
    }

    #[test]
    fn 左が明るいと全ビットが立つ() {
        // 左ほど明るい＝どの隣どうしでも left > right。
        let value = d_hash_from_luma(luma(|_, col| (200 - col * 20) as u8)).unwrap();
        assert_eq!(value, "ffffffffffffffff");
    }

    #[test]
    fn 全体を明るくしても値が変わらない() {
        // **隣との差だけを見るので、露出が違っても同じ構図なら同じ値になる。**
        let dark = d_hash_from_luma(luma(|row, col| (10 + row * 2 + col * 3) as u8)).unwrap();
        let bright = d_hash_from_luma(luma(|row, col| (90 + row * 2 + col * 3) as u8)).unwrap();
        assert_eq!(dark, bright);
    }

    #[test]
    fn 違う構図は距離が開く() {
        let left = d_hash_from_luma(luma(|_, col| (200 - col * 20) as u8)).unwrap();
        let right = d_hash_from_luma(luma(|_, col| (10 + col * 20) as u8)).unwrap();
        assert_eq!(hash_distance(left, right), 64, "正反対なら全ビット違う");
    }

    #[test]
    fn 少し違うだけなら距離は小さい() {
        let base = luma(|row, col| (10 + row * 2 + col * 3) as u8);
        let mut tweaked = base.clone();
        // 1 か所だけ隣との大小を反転させる。
        tweaked[0] = 255;
        let a = d_hash_from_luma(base).unwrap();
        let b = d_hash_from_luma(tweaked).unwrap();
        let distance = hash_distance(a, b);
        assert!(distance > 0 && distance <= 2, "距離が {distance} は大きすぎる");
    }

    #[test]
    fn 読めない写真どうしを同一と見なさない() {
        // 0 を返すと、**読めない写真が全部 1 つのまとまりに落ちる。**
        assert_eq!(hash_distance("".into(), "".into()), 64);
        assert_eq!(hash_distance("zzz".into(), "zzz".into()), 64);
    }

    #[test]
    fn 同じ値の距離は零() {
        assert_eq!(hash_distance("ffffffffffffffff".into(), "ffffffffffffffff".into()), 0);
        assert_eq!(hash_distance("0000000000000000".into(), "0000000000000001".into()), 1);
    }

    #[test]
    fn 時間が近くても見た目が違えばまとめない() {
        let pair = BurstPair {
            left: photo("a.jpg", 1000, "0000000000000000"),
            right: photo("b.jpg", 1500, "ffffffffffffffff"),
        };
        assert!(!is_same_burst(pair, threshold()));
    }

    #[test]
    fn 見た目が同じでも時間が離れていればまとめない() {
        let pair = BurstPair {
            left: photo("a.jpg", 1000, "0000000000000000"),
            right: photo("b.jpg", 100_000, "0000000000000000"),
        };
        assert!(!is_same_burst(pair, threshold()));
    }

    #[test]
    fn 撮影時刻が読めないものはまとめない() {
        let mut left = photo("a.jpg", 1000, "0000000000000000");
        left.captured_at = None;
        let pair = BurstPair {
            left,
            right: photo("b.jpg", 1500, "0000000000000000"),
        };
        assert!(!is_same_burst(pair, threshold()));
    }

    #[test]
    fn 版が違うハッシュは比べない() {
        let mut left = photo("a.jpg", 1000, "0000000000000000");
        left.d_hash_version = 1;
        let pair = BurstPair {
            left,
            right: photo("b.jpg", 1500, "0000000000000000"),
        };
        assert!(!is_same_burst(pair, threshold()));
    }

    #[test]
    fn 連続するペアが基準を満たす区間が一つのまとまりになる() {
        let photos = vec![
            photo("1.jpg", 1000, "0000000000000000"),
            photo("2.jpg", 2000, "0000000000000001"),
            photo("3.jpg", 3000, "0000000000000003"),
            // ここで時間が飛ぶ
            photo("4.jpg", 60_000, "0000000000000003"),
            photo("5.jpg", 61_000, "0000000000000007"),
        ];
        let groups = group_bursts(photos, threshold(), vec![]);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].members, vec!["1.jpg", "2.jpg", "3.jpg"]);
        assert_eq!(groups[1].members, vec!["4.jpg", "5.jpg"]);
        assert_eq!(groups[0].representative, "1.jpg");
    }

    #[test]
    fn 手で切った所は基準より優先される() {
        let photos = vec![
            photo("1.jpg", 1000, "0000000000000000"),
            photo("2.jpg", 2000, "0000000000000001"),
            photo("3.jpg", 3000, "0000000000000003"),
        ];
        let overrides = vec![PairOverride {
            left: "1.jpg".into(),
            right: "2.jpg".into(),
            decision: "split".into(),
        }];
        let groups = group_bursts(photos, threshold(), overrides);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].members, vec!["1.jpg"]);
        assert_eq!(groups[1].members, vec!["2.jpg", "3.jpg"]);
    }

    #[test]
    fn 手で繋いだ所も基準より優先される() {
        let photos = vec![
            photo("1.jpg", 1000, "0000000000000000"),
            // 基準では切れる距離
            photo("2.jpg", 90_000, "ffffffffffffffff"),
        ];
        let overrides = vec![PairOverride {
            left: "1.jpg".into(),
            right: "2.jpg".into(),
            decision: "join".into(),
        }];
        let groups = group_bursts(photos, threshold(), overrides);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].members, vec!["1.jpg", "2.jpg"]);
    }

    // ---- 次のラウンド ----

    /// 全部残す形でラウンドを終わらせる。
    fn run_all(mut session: Session) -> Session {
        while !session.finished {
            let all = session.current.clone();
            session = advance(session, all);
        }
        session
    }

    #[test]
    fn 次のラウンドは通ったものだけで始まる() {
        let photos = plain(&["1", "2", "3", "4"]);
        let first = start_round(photos.clone(), 2, 0, false, threshold(), vec![]);
        let first = advance(first, vec!["1".into()]);
        let first = advance(first, vec!["3".into()]);

        let second = next_round(first, photos, false, threshold(), vec![]).unwrap();
        assert_eq!(second.round, 2);
        assert_eq!(second.target_star, 1);
        // 通った 2 枚だけ。落ちたものは出てこない。
        assert_eq!(second.current, vec!["1", "3"]);
        assert!(second.queue.is_empty());
    }

    #[test]
    fn 次のラウンドでも星は引き継がれる() {
        let photos = plain(&["1", "2", "3", "4"]);
        let first = start_round(photos.clone(), 2, 0, false, threshold(), vec![]);
        let first = advance(first, vec!["1".into()]);
        let first = advance(first, vec!["3".into()]);

        let second = next_round(first, photos, false, threshold(), vec![]).unwrap();
        assert_eq!(second.ratings["1"], 1);
        assert_eq!(second.ratings["3"], 1);
        // **落ちたものの星も消えない。** ★0 として残る。
        assert_eq!(second.ratings["2"], 0);
        assert_eq!(second.ratings["4"], 0);
    }

    #[test]
    fn 畳まれた仲間の星も次のラウンドに残る() {
        // 1.jpg と 2.jpg が連写。代表は 1.jpg。
        let photos = vec![
            photo("1.jpg", 0, "0000000000000000"),
            photo("2.jpg", 1000, "0000000000000001"),
            photo("3.jpg", 500_000, "ffffffffffffffff"),
            photo("4.jpg", 600_000, "0f0f0f0f0f0f0f0f"),
        ];
        let first = start_round(photos.clone(), 2, 0, true, threshold(), vec![]);
        assert_eq!(first.current, vec!["1.jpg", "3.jpg"]);
        let first = advance(first, vec!["1.jpg".into()]);
        let first = run_all(first);

        let second = next_round(first, photos, true, threshold(), vec![]).unwrap();
        // 仲間は画面に出ないが、星は代表と同じだけ動いている。
        assert_eq!(second.ratings["2.jpg"], 1);
        assert!(!second.queue.contains(&"2.jpg".to_string()));
        assert!(!second.current.contains(&"2.jpg".to_string()));
    }

    #[test]
    fn 通ったものが一枚なら次のラウンドは無い() {
        let photos = plain(&["1", "2"]);
        let first = start_round(photos.clone(), 2, 0, false, threshold(), vec![]);
        let first = advance(first, vec!["1".into()]);
        assert!(next_round(first, photos, false, threshold(), vec![]).is_none());
    }

    #[test]
    fn 星が上限なら次のラウンドは無い() {
        let photos = plain(&["1", "2", "3", "4"]);
        // target_star が 4 なら次は 5。5 は上限なのでこれ以上は進めない。
        let first = start_round(photos.clone(), 2, MAX_STAR - 1, false, threshold(), vec![]);
        let first = run_all(first);
        assert!(next_round(first, photos, false, threshold(), vec![]).is_none());
    }

    #[test]
    fn 次のラウンドは撮影順を保つ() {
        let photos = plain(&["1", "2", "3", "4", "5", "6"]);
        let first = start_round(photos.clone(), 2, 0, false, threshold(), vec![]);
        // わざと後ろから選び、survivors の並びを撮影順と食い違わせる。
        let first = advance(first, vec!["2".into()]);
        let first = advance(first, vec!["4".into()]);
        let first = advance(first, vec!["5".into()]);

        let second = next_round(first, photos, false, threshold(), vec![]).unwrap();
        let order: Vec<String> = second
            .current
            .iter()
            .chain(second.queue.iter())
            .cloned()
            .collect();
        assert_eq!(order, vec!["2", "4", "5"]);
    }

    #[test]
    fn 次のラウンドでは戻せない() {
        let photos = plain(&["1", "2", "3", "4"]);
        let first = start_round(photos.clone(), 2, 0, false, threshold(), vec![]);
        let first = advance(first, vec!["1".into()]);
        let first = advance(first, vec!["3".into()]);

        let second = next_round(first, photos, false, threshold(), vec![]).unwrap();
        // 前のラウンドの判断は残っていない。またぐと round と星が合わなくなる。
        assert!(second.history.is_empty());
    }

    // ---- 升目平均での縮小 ----

    /// 左半分が暗く右半分が明るい絵。横の差分は 1 か所だけ立つ。
    fn split_image(width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::with_capacity((width * height) as usize);
        for _ in 0..height {
            for x in 0..width {
                out.push(if x < width / 2 { 20 } else { 200 });
            }
        }
        out
    }

    #[test]
    fn 大きさが足りなければ作らない() {
        assert!(d_hash_from_gray(vec![0; 8 * 8], 8, 8).is_none());
        assert!(d_hash_from_gray(vec![0; 9 * 7], 9, 7).is_none());
    }

    #[test]
    fn 画素数と大きさが合わなければ作らない() {
        assert!(d_hash_from_gray(vec![0; 10], 9, 8).is_none());
    }

    #[test]
    fn 同じ絵を違う大きさで渡しても同じ値になる() {
        // **これが升目平均にした理由。** 縮小率が変わっても値が動かない。
        let small = d_hash_from_gray(split_image(90, 80), 90, 80).unwrap();
        let large = d_hash_from_gray(split_image(900, 800), 900, 800).unwrap();
        assert_eq!(small, large);
    }

    #[test]
    fn 一画素の汚れで値が動かない() {
        // 大きな絵の 1 画素を反転させても、升目の平均はほとんど動かない。
        let clean = split_image(180, 160);
        let mut dirty = clean.clone();
        dirty[100 * 180 + 100] = 255 - dirty[100 * 180 + 100];
        let a = d_hash_from_gray(clean, 180, 160).unwrap();
        let b = d_hash_from_gray(dirty, 180, 160).unwrap();
        assert_eq!(hash_distance(a, b), 0);
    }

    #[test]
    fn 全体が明るくなっても値が動かない() {
        // 隣との差分を見ているので、明るさを一律に足しても変わらない。
        let base = split_image(90, 80);
        let brighter: Vec<u8> = base.iter().map(|v| v.saturating_add(40)).collect();
        let a = d_hash_from_gray(base, 90, 80).unwrap();
        let b = d_hash_from_gray(brighter, 90, 80).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn 違う絵は距離が開く() {
        let split = d_hash_from_gray(split_image(90, 80), 90, 80).unwrap();
        // 縞模様。升目ごとに明暗が入れ替わる。
        let mut stripes = Vec::new();
        for _ in 0..80 {
            for x in 0..90 {
                stripes.push(if (x / 10) % 2 == 0 { 20 } else { 200 });
            }
        }
        let striped = d_hash_from_gray(stripes, 90, 80).unwrap();
        assert!(hash_distance(split, striped) > 4);
    }

    // ---- 代表の選び直し ----

    /// 1.jpg と 2.jpg が連写、3.jpg と 4.jpg は別。
    fn burst_photos() -> Vec<PhotoRef> {
        vec![
            photo("1.jpg", 0, "0000000000000000"),
            photo("2.jpg", 1000, "0000000000000001"),
            photo("3.jpg", 500_000, "ffffffffffffffff"),
            photo("4.jpg", 900_000, "0f0f0f0f0f0f0f0f"),
        ]
    }

    #[test]
    fn 代表を仲間に差し替えられる() {
        let session = start_round(burst_photos(), 2, 0, true, threshold(), vec![]);
        assert_eq!(session.current, vec!["1.jpg", "3.jpg"]);

        let after = set_representative(session, "1.jpg".into(), "2.jpg".into()).unwrap();
        // 画面に出るのが入れ替わる。並びの位置は動かさない。
        assert_eq!(after.current, vec!["2.jpg", "3.jpg"]);
        // 仲間の顔ぶれはそのまま。鍵だけ変わる。
        assert_eq!(after.members["2.jpg"], vec!["1.jpg", "2.jpg"]);
        assert!(!after.members.contains_key("1.jpg"));
    }

    #[test]
    fn 代表を変えても星は動かない() {
        let session = start_round(burst_photos(), 2, 0, true, threshold(), vec![]);
        let before = session.ratings.clone();
        let after = set_representative(session, "1.jpg".into(), "2.jpg".into()).unwrap();
        // **見せ方を変えただけ。** 残すかどうかは advance が決める。
        assert_eq!(after.ratings, before);
    }

    #[test]
    fn 差し替えた代表を残すと仲間にも星が付く() {
        let session = start_round(burst_photos(), 2, 0, true, threshold(), vec![]);
        let session = set_representative(session, "1.jpg".into(), "2.jpg".into()).unwrap();
        let after = advance(session, vec!["2.jpg".into()]);
        assert_eq!(after.ratings["2.jpg"], 1);
        // 元の先頭も仲間として同じだけ動く。
        assert_eq!(after.ratings["1.jpg"], 1);
        // survivors に入るのは代表だけ。畳んだ意味が消えないように。
        assert_eq!(after.survivors, vec!["2.jpg"]);
    }

    #[test]
    fn まとまりでないものは差し替えられない() {
        let session = start_round(burst_photos(), 2, 0, true, threshold(), vec![]);
        assert!(set_representative(session, "3.jpg".into(), "4.jpg".into()).is_none());
    }

    #[test]
    fn 仲間でないものは代表にできない() {
        let session = start_round(burst_photos(), 2, 0, true, threshold(), vec![]);
        assert!(set_representative(session, "1.jpg".into(), "3.jpg".into()).is_none());
    }

    #[test]
    fn 見終わったまとまりは差し替えられない() {
        let session = start_round(burst_photos(), 2, 0, true, threshold(), vec![]);
        // 1.jpg と 3.jpg のグループを確定させて先へ進める。
        let session = advance(session, vec!["1.jpg".into()]);
        // **済んだ判断は動かさない。** 動かすと history と食い違う。
        assert!(set_representative(session, "1.jpg".into(), "2.jpg".into()).is_none());
    }

    #[test]
    fn 同じものを指しても壊れない() {
        let session = start_round(burst_photos(), 2, 0, true, threshold(), vec![]);
        let after = set_representative(session.clone(), "1.jpg".into(), "1.jpg".into()).unwrap();
        assert_eq!(after.current, session.current);
        assert_eq!(after.members["1.jpg"], session.members["1.jpg"]);
    }

    #[test]
    fn まだ見ていないまとまりも差し替えられる() {
        // group_size 1 相当にはできないので、後ろに連写を置く。
        let photos = vec![
            photo("1.jpg", 0, "ffffffffffffffff"),
            photo("2.jpg", 500_000, "0f0f0f0f0f0f0f0f"),
            photo("3.jpg", 900_000, "0000000000000000"),
            photo("4.jpg", 901_000, "0000000000000001"),
        ];
        let session = start_round(photos, 2, 0, true, threshold(), vec![]);
        assert_eq!(session.current, vec!["1.jpg", "2.jpg"]);
        assert_eq!(session.queue, vec!["3.jpg"]);

        let after = set_representative(session, "3.jpg".into(), "4.jpg".into()).unwrap();
        assert_eq!(after.queue, vec!["4.jpg"]);
    }

    // ---- まとめ直し ----

    /// 1,2 が連写。3,4 は離れている。5,6 も連写。
    fn regroup_photos() -> Vec<PhotoRef> {
        vec![
            photo("1.jpg", 0, "0000000000000000"),
            photo("2.jpg", 1000, "0000000000000001"),
            photo("3.jpg", 500_000, "ffffffffffffffff"),
            photo("4.jpg", 900_000, "0f0f0f0f0f0f0f0f"),
            photo("5.jpg", 1_400_000, "00ff00ff00ff00ff"),
            photo("6.jpg", 1_401_000, "00ff00ff00ff00fe"),
        ]
    }

    fn split(left: &str, right: &str) -> PairOverride {
        PairOverride { left: left.into(), right: right.into(), decision: "split".into() }
    }

    fn join(left: &str, right: &str) -> PairOverride {
        PairOverride { left: left.into(), right: right.into(), decision: "join".into() }
    }

    #[test]
    fn まとまりを解くとその場で組が増える() {
        let photos = regroup_photos();
        let session = start_round(photos.clone(), 2, 0, true, threshold(), vec![]);
        // 1+2 / 3 / 4 / 5+6 の 4 組
        assert_eq!(session.queue.len() + session.current.len(), 4);

        let after = regroup(session, photos, true, threshold(), vec![split("1.jpg", "2.jpg")]);
        // 1 / 2 / 3 / 4 / 5+6 の 5 組
        assert_eq!(after.queue.len() + after.current.len(), 5);
        assert!(!after.members.contains_key("1.jpg"));
    }

    #[test]
    fn 繋ぐとその場で組が減る() {
        let photos = regroup_photos();
        let session = start_round(photos.clone(), 2, 0, true, threshold(), vec![]);
        let after = regroup(session, photos, true, threshold(), vec![join("3.jpg", "4.jpg")]);
        assert_eq!(after.queue.len() + after.current.len(), 3);
        assert_eq!(after.members["3.jpg"], vec!["3.jpg", "4.jpg"]);
    }

    #[test]
    fn 組み直しても決めた写真は出てこない() {
        let photos = regroup_photos();
        let session = start_round(photos.clone(), 2, 0, true, threshold(), vec![]);
        // 1+2 と 3 の組を確定して先へ進む。
        assert_eq!(session.current, vec!["1.jpg", "3.jpg"]);
        let session = advance(session, vec!["1.jpg".into()]);

        let after = regroup(session, photos, true, threshold(), vec![join("5.jpg", "6.jpg")]);
        let showing: Vec<String> = after.current.iter().chain(after.queue.iter()).cloned().collect();
        // **決めた 1,2,3 はもう出ない。** 残りは 4 と 5+6。
        assert!(!showing.contains(&"1.jpg".to_string()));
        assert!(!showing.contains(&"2.jpg".to_string()));
        assert!(!showing.contains(&"3.jpg".to_string()));
        assert_eq!(showing, vec!["4.jpg", "5.jpg"]);
    }

    #[test]
    fn 組み直しても星は残る() {
        let photos = regroup_photos();
        let session = start_round(photos.clone(), 2, 0, true, threshold(), vec![]);
        let session = advance(session, vec!["1.jpg".into()]);
        assert_eq!(session.ratings["1.jpg"], 1);
        assert_eq!(session.ratings["2.jpg"], 1);

        let after = regroup(session, photos, true, threshold(), vec![split("5.jpg", "6.jpg")]);
        // **進んだ分は星として残る。** これが「その場で組み直す」の要点。
        assert_eq!(after.ratings["1.jpg"], 1);
        assert_eq!(after.ratings["2.jpg"], 1);
        assert_eq!(after.survivors, vec!["1.jpg"]);
    }

    #[test]
    fn 組み直しても決めた分は戻せる() {
        let photos = regroup_photos();
        let session = start_round(photos.clone(), 2, 0, true, threshold(), vec![]);
        let session = advance(session, vec!["1.jpg".into()]);

        let after = regroup(session, photos, true, threshold(), vec![split("5.jpg", "6.jpg")]);
        // 決めたまとまりの仲間を控えているので、戻すと星も正しく戻る。
        let undone = undo(after);
        assert_eq!(undone.ratings["1.jpg"], 0);
        assert_eq!(undone.ratings["2.jpg"], 0);
        assert!(undone.survivors.is_empty());
    }

    #[test]
    fn 手で選んだ代表は顔ぶれが同じなら残る() {
        let photos = regroup_photos();
        let session = start_round(photos.clone(), 2, 0, true, threshold(), vec![]);
        let session = set_representative(session, "1.jpg".into(), "2.jpg".into()).unwrap();

        // 別のところを切っても、1+2 の顔ぶれは変わらない。
        let after = regroup(session, photos, true, threshold(), vec![split("5.jpg", "6.jpg")]);
        assert!(after.members.contains_key("2.jpg"));
        assert!(after.current.contains(&"2.jpg".to_string()));
    }

    #[test]
    fn 組み直しはラウンドと星の段を動かさない() {
        let photos = regroup_photos();
        let session = start_round(photos.clone(), 2, 2, true, threshold(), vec![]);
        let after = regroup(session, photos, true, threshold(), vec![split("1.jpg", "2.jpg")]);
        assert_eq!(after.round, 1);
        assert_eq!(after.target_star, 2);
    }

    // ---- 基準の学習 ----

    fn answer(distance: u32, same: bool) -> BurstAnswer {
        BurstAnswer { distance, same }
    }

    #[test]
    fn 答えが無ければ既定値のまま() {
        assert_eq!(learn_distance(vec![], 9), 9);
    }

    #[test]
    fn 同じと別のあいだに境目を置く() {
        // 6 までは同じ、14 からは別 → あいだの 10
        let answers = vec![answer(4, true), answer(6, true), answer(14, false), answer(18, false)];
        assert_eq!(learn_distance(answers, 9), 10);
    }

    #[test]
    fn 同じしか無ければそこまでは繋ぐ() {
        let answers = vec![answer(5, true), answer(13, true)];
        assert_eq!(learn_distance(answers, 9), 13);
    }

    #[test]
    fn 別しか無ければその手前で切る() {
        let answers = vec![answer(11, false), answer(20, false)];
        assert_eq!(learn_distance(answers, 9), 10);
    }

    #[test]
    fn 食い違ったら切る側に倒す() {
        // 12 を「同じ」、8 を「別」と答えた。素直な境目が無い。
        // **繋ぎすぎるより切りすぎる方が安全**なので 7。
        let answers = vec![answer(12, true), answer(8, false)];
        assert_eq!(learn_distance(answers, 9), 7);
    }

    #[test]
    fn 極端な値は基準にしない() {
        // 全部「別」と答えても 0 にはしない。何も繋がらない基準は基準でない。
        assert_eq!(learn_distance(vec![answer(1, false)], 9), 2);
        // 全部「同じ」でも上限で止める。何でも繋がると畳んだ意味が消える。
        assert_eq!(learn_distance(vec![answer(60, true)], 9), 24);
    }

    #[test]
    fn 学習した値がそのまままとめに効く() {
        // 距離 10 の 2 枚。既定の 9 ではまとまらないが、学習で 10 になれば繋がる。
        let photos = vec![
            photo("1.jpg", 0, "0000000000000000"),
            photo("2.jpg", 1000, "00000000000003ff"), // 10 ビット違い
        ];
        assert_eq!(hash_distance("0000000000000000".into(), "00000000000003ff".into()), 10);

        let strict = BurstThreshold { window_ms: 4000, distance: 9, d_hash_version: 2 };
        assert_eq!(group_bursts(photos.clone(), strict, vec![]).len(), 2);

        let learned = learn_distance(vec![answer(6, true), answer(14, false)], 9);
        let loose = BurstThreshold { window_ms: 4000, distance: learned, d_hash_version: 2 };
        assert_eq!(group_bursts(photos, loose, vec![]).len(), 1);
    }

    // ---- 枚数の変更 ----

    #[test]
    fn 枚数を増やすと次から補う() {
        let session = round(&["1", "2", "3", "4", "5", "6"], 2);
        assert_eq!(session.current, vec!["1", "2"]);
        let after = resize(session, 4);
        assert_eq!(after.current, vec!["1", "2", "3", "4"]);
        assert_eq!(after.queue, vec!["5", "6"]);
        assert_eq!(after.group_size, 4);
    }

    #[test]
    fn 枚数を減らすと次へ押し出す() {
        let session = round(&["1", "2", "3", "4", "5", "6"], 4);
        assert_eq!(session.current, vec!["1", "2", "3", "4"]);
        let after = resize(session, 2);
        assert_eq!(after.current, vec!["1", "2"]);
        // **押し出した分は捨てない。** 次のグループの先頭に戻る。
        assert_eq!(after.queue, vec!["3", "4", "5", "6"]);
    }

    #[test]
    fn 枚数を変えても見終わった分は動かない() {
        let session = round(&["1", "2", "3", "4", "5", "6"], 2);
        let session = advance(session, vec!["1".into()]);
        let after = resize(session, 3);
        assert_eq!(after.history.len(), 1);
        assert_eq!(after.survivors, vec!["1"]);
        // 残りは 3,4,5,6 の 4 枚。3 枚ずつなので 3,4,5 が出る。
        assert_eq!(after.current, vec!["3", "4", "5"]);
        assert_eq!(after.queue, vec!["6"]);
    }

    #[test]
    fn 枚数は二枚を下回らない() {
        let session = round(&["1", "2", "3", "4"], 4);
        let after = resize(session, 1);
        assert_eq!(after.group_size, 2);
    }
}
