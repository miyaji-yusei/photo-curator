//! 連写候補の絞り込み（select_burst_candidates・pair_eligibility）。
//! U54 で lib.rs から移した（中身は変えていない）。

use super::*;

// ---------------------------------------------------------------------------
// 連写候補の絞り込み
// ---------------------------------------------------------------------------

/// 候補判定に必要な最小限。撮影時刻順に並んでいることが前提。
#[derive(Clone, Debug)]
pub struct CandidateInput {
    pub id: String,
    pub captured_at: i64,
    pub source: TimestampSource,
}

#[derive(Debug)]
pub struct CandidateSelection {
    pub ids: HashSet<String>,
    /// 実際に使った時間窓。自動縮小が働くと `BURST_WINDOW_MS` より小さくなる。
    pub window_ms: i64,
    pub ratio: f64,
    pub narrowed: bool,
    /// 弱い根拠（mtime / 不明）同士だったために除外したペアの数。
    pub weak_pairs_skipped: usize,
}

/// 隣接ペアが連写候補になりうるか。時間と根拠の強さだけで決め、構図は見ない。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairEligibility {
    Eligible,
    OutOfWindow,
    /// 撮影時刻の根拠が両側とも弱い。時間が近いだけでは連写とみなさない。
    WeakEvidence,
}

/// 候補判定の一次審査。`candidates_within`（解析対象の絞り込み）と
/// 以前は閾値学習の出題元も同じ規則を使っていた（いまは core が判断する）。
/// 二重実装すると、片方だけ直したときに学習と本番でズレる。
pub(crate) fn pair_eligibility(
    left: &CandidateInput,
    right: &CandidateInput,
    window_ms: i64,
) -> PairEligibility {
    let delta = right.captured_at - left.captured_at;
    if !(0..=window_ms).contains(&delta) {
        return PairEligibility::OutOfWindow;
    }
    // mtime はコピーやダウンロードで簡単に揃う。ここを通すと、一括ダウンロード
    // したフォルダが丸ごと候補になる。
    if left.source.is_weak() && right.source.is_weak() {
        return PairEligibility::WeakEvidence;
    }
    PairEligibility::Eligible
}

pub(crate) fn candidates_within(records: &[CandidateInput], window_ms: i64) -> (HashSet<String>, usize) {
    let mut ids = HashSet::new();
    let mut weak_pairs_skipped = 0usize;
    for pair in records.windows(2) {
        let [left, right] = pair else { continue };
        match pair_eligibility(left, right, window_ms) {
            PairEligibility::OutOfWindow => continue,
            PairEligibility::WeakEvidence => {
                weak_pairs_skipped += 1;
                continue;
            }
            PairEligibility::Eligible => {}
        }
        ids.insert(left.id.clone());
        ids.insert(right.id.clone());
    }
    (ids, weak_pairs_skipped)
}

/// 連写候補を選ぶ。候補が**多すぎる**うえに候補率も高いときだけ、時間窓を
/// 半分ずつ詰める。
///
/// 「時間が近い写真だけハッシュする」最適化は、撮影間隔がほぼ全て窓の内側に
/// 収まるフォルダでは原理的に効かない（実データ271枚では 98.5% が候補）。
///
/// Routine 2 は候補率だけで縮小を判断していたが、それでは密に撮影された正常な
/// データにも発火し、実測で連写グループを 52 → 16 に減らしていた。得たものは
/// 194 ms、失ったものは 36 グループで、Step 4 が 1枚 1.66 ms を実現した今では
/// 割に合わない。`CANDIDATE_COUNT_LIMIT` を併せて課し、**実コストが本当に
/// 問題になる規模でだけ**縮小するようにしてある。
/// mtime だけを根拠にした爆発は `candidates_within` の弱ペア除外が既に
/// 潰しているので（合成5000枚で候補 0%）、この変更で退行はしない。
pub(crate) fn select_burst_candidates(records: &[CandidateInput]) -> CandidateSelection {
    let total = records.len();
    let ratio_of = |ids: &HashSet<String>| {
        if total == 0 {
            0.0
        } else {
            ids.len() as f64 / total as f64
        }
    };
    let too_many = |ids: &HashSet<String>| {
        ids.len() > CANDIDATE_COUNT_LIMIT && ratio_of(ids) > CANDIDATE_RATIO_LIMIT
    };
    let mut window_ms = BURST_WINDOW_MS;
    let (mut ids, mut weak_pairs_skipped) = candidates_within(records, window_ms);
    let mut narrowed = false;
    while too_many(&ids) && window_ms > MIN_BURST_WINDOW_MS {
        window_ms = (window_ms / 2).max(MIN_BURST_WINDOW_MS);
        narrowed = true;
        let next = candidates_within(records, window_ms);
        ids = next.0;
        weak_pairs_skipped = next.1;
    }
    CandidateSelection {
        ratio: ratio_of(&ids),
        ids,
        window_ms,
        narrowed,
        weak_pairs_skipped,
    }
}
