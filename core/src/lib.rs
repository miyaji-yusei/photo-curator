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
// 選別
// ---------------------------------------------------------------------------

use std::collections::HashMap;

/// 星の上限。これに達した写真は以降のラウンドに出ない。
const MAX_STAR: i32 = 5;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Decision {
    pub group: Vec<String>,
    pub chosen: Vec<String>,
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

    next.history.push(Decision { group, chosen });
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

    fill(&mut next);
    next
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
}
