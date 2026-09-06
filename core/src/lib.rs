//! Photo Curator の中核。**判断が絡むものだけを置く。**
//!
//! バイトを読む・画像をデコードする・画面に出すのは各環境の仕事。
//! ここに置くのは「同じ入力なら同じ答えでなければ困るもの」だけ。
//!
//! いまは連写のまとめ方が Rust（PC）と TypeScript（Web）に二重実装されていて、
//! 片方だけ直る不具合が実際に起きた。作り直しでここを 1 つに畳む。

uniffi::include_scaffolding!("core");

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
