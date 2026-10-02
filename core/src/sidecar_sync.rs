//! サイドカー同期の判断（U33・設計書「サイドカー同期の調査と直し方の設計」の PR-3）。
//!
//! 選別状況（星・セッション・手直し・学習した境目）が、同期の取り込みで黙って
//! 消える不具合の根本対策。**判断はここ 1 か所に置き、PC・Web（wasm）と Android
//! （UniFFI）が同じ規則で動く。** 読み書き（NAS・ファイル）とロックは各環境の仕事。
//!
//! 柱は 3 つ:
//! 1. 選別状況を正規化して**意味で比べる**（`canonical_judgement`・`judgement_equivalent`・
//!    比較キー `judgement_key`）。JSON の並び・空白・`updatedAt`・`updatedBy`・`writeId` が
//!    違っても、中身が同じなら「同じ」。端末が変わったかどうかも、印（dirty）ではなく
//!    「見た版の比較キーと今の比較キーが違うか」で決める。
//! 2. **未着手**（`is_untouched`）を厳密に決め、片方が未着手なら確認なしにもう片方へ合わせる。
//! 3. 開き方の判断 `sidecar_plan`（何もしない／書く／取り込む／確認）。両方が着手済みで
//!    中身が違うときだけ確認する。**端末の分を確認なしに捨てることはない。**
//!
//! 用語: 「選別状況」＝上の 4 つ。「比較キー」＝選別状況の正規形から作る値。
//! 画像の特徴値（dHash）の「ハッシュ値」とは関係が無い。

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use crate::{Decision, PairOverride, Session, Sidecar, Topped, MAX_STAR};

/// 新しく書く catalog.json の版。これより新しい版は読むだけにして書かない。
pub const SIDECAR_VERSION: i32 = 2;

/// `keyBase` の値。写真の鍵が「選んだフォルダからの相対・`/` 区切り」であること。
pub const KEY_BASE_FOLDER: &str = "folder";

/// 系統（`lineage`）に控える版の数の上限。
const LINEAGE_LIMIT: usize = 32;

/// 比較キーの形の版。正規形の作り方を変えたら上げる（古いキーとは必ず違う値になる）。
const JUDGEMENT_KEY_PREFIX: &str = "j1:";

/// v2 で足した項目を読むときの受け口。**型が違う値は読み捨てる**（None にする）。
///
/// ほかの版のアプリや手で直したファイルに変な値が入っていても、catalog.json 全体を
/// 「壊れている」にしない（壊れている扱いだと、ずっと同期できなくなる）。
pub fn lenient<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let value: serde_json::Value = serde::Deserialize::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).ok())
}

// ---------------------------------------------------------------------------
// 型
// ---------------------------------------------------------------------------

/// 判断と画面のための要約。catalog.json の `progress` にも書く。
///
/// **Session から作り直せる値で、自動の判断には使わない**（ダイアログの 1 行と、
/// どちらが進んでいるかの印だけ）。どの項目も省略できる。
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SidecarProgress {
    /// 着手済みか（`!is_untouched`）。
    #[serde(default)]
    pub started: bool,
    /// ROUND。セッションが無ければ 0。
    #[serde(default)]
    pub round: u32,
    #[serde(default)]
    pub finished: bool,
    /// この ROUND で決めた組の数。
    #[serde(default)]
    pub decided: u32,
    /// まだ見ていない写真の数（連写の仲間も数える）。
    #[serde(default)]
    pub remaining: u32,
    /// ★1 以上の数。
    #[serde(default)]
    pub starred: u32,
    /// 写真の数（分かるときだけ）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u32>,
    /// 手直しの数。
    #[serde(default)]
    pub overrides: u32,
    /// 連写の境目を学習したか。
    #[serde(default)]
    pub learned: bool,
}

/// 写真 1 枚の★（★1 以上だけを持つ）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PhotoStar {
    pub path: String,
    pub rating: i32,
}

/// 連写のまとまり（代表と仲間。仲間は並べ替えて重複なし）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MemberGroup {
    pub representative: String,
    pub members: Vec<String>,
}

/// 1 手の判断（組と選んだものは集合として並べ替える。戻すための控えは持たない）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct JudgementDecision {
    pub group: Vec<String>,
    pub chosen: Vec<String>,
    /// ★5 で確定した 1 枚。
    pub topped: Option<String>,
}

/// セッションの正規形。**1 組の枚数（group_size）は持たない**（表示の設定。§6 Q12）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct JudgementSession {
    pub round: u32,
    pub target_star: i32,
    pub finished: bool,
    /// まだ見ていない代表の並び（`current ++ queue`。順序あり）。
    pub remaining: Vec<String>,
    /// このラウンドを通ったもの（並べ替えた集合）。
    pub survivors: Vec<String>,
    /// まとまり（仲間が 2 枚以上のものだけ。代表の順）。
    pub members: Vec<MemberGroup>,
    pub history: Vec<JudgementDecision>,
}

/// 選別状況の正規形（設計書 §4.2.1）。**ここに無いものは比べない。**
///
/// 写真の鍵は「選んだフォルダからの相対・`/` 区切り・NFC」。呼ぶ側は、端末の中の鍵を
/// 先に `sidecar_keys_to_folder` などでフォルダ形式にしておく。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Judgement {
    /// ★1 以上の写真（鍵の順）。★0 と「鍵が無い」は同じ扱い。
    pub stars: Vec<PhotoStar>,
    pub session: Option<JudgementSession>,
    /// 手直し（`left`・`right` の順。同じ 2 枚は先に出たものだけ＝core の group_bursts と同じ）。
    pub overrides: Vec<PairOverride>,
    pub burst_distance: Option<u32>,
    /// やり直しの世代。
    pub epoch: Option<String>,
    /// その端末の記録にある写真（★0 も含む。セッションの `ratings` と `photos` の鍵。鍵の順）。
    /// 積集合（D）で「その端末の記録に無い写真＝未判定」を見分けるためだけに使う（D3）。
    /// **意味が同じかの判断と比較キーには入れない**（端末ごとに対象の拡張子が違うので、
    /// ★0 の顔ぶれの違いで警告を出さない）。None は「分からない」＝全部を記録にあると見なす。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub known: Option<Vec<String>>,
}

/// 意味が同じか。`known`（記録の顔ぶれ）は比べない。
impl PartialEq for Judgement {
    fn eq(&self, other: &Self) -> bool {
        self.stars == other.stars
            && self.session == other.session
            && self.overrides == other.overrides
            && self.burst_distance == other.burst_distance
            && self.epoch == other.epoch
    }
}

impl Eq for Judgement {}

/// 比較キーに使う形（`known` を除いた Judgement と同じ JSON になる。並びも同じ）。
#[derive(serde::Serialize)]
struct JudgementKeyView<'a> {
    stars: &'a Vec<PhotoStar>,
    session: &'a Option<JudgementSession>,
    overrides: &'a Vec<PairOverride>,
    burst_distance: &'a Option<u32>,
    epoch: &'a Option<String>,
}

/// 端末の控え。最後に読んだ／書いた版。**今の seenAt/seenBy/dirty の代わり。**
///
/// - `token`: 版の見分け（`sidecar_token`）。一度も見ていなければ空。
/// - `key`: その版を読んだ／書いたときの、選別状況の比較キー。分からなければ空
///   （空なら「端末に変更あり」と見なす。移行で古い dirty が true のときに使う）。
/// - `epoch`: その版のやり直しの世代。
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SeenRecord {
    pub token: String,
    pub key: String,
    pub epoch: Option<String>,
}

/// 何もしないときの理由。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SettledReason {
    /// NAS に無く、端末も未着手。
    Nothing,
    /// 意味が同じ（文字列は違ってもよい）。**警告は出さない。** 控えを NAS の版に進める。
    Same,
    /// 見た版のままで、端末も変わっていない。
    NoChange,
    /// 「この端末の状況を残す」を選んだあと。自動では書かない。
    Detached,
    /// 書けない共有。端末の分はそのまま（「この端末だけの結果」）。
    ReadOnly,
    /// NAS の版が新しすぎる（知らない形）。読むだけにして書かない。
    NewerVersion,
}

/// 書くときの理由。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PushReason {
    /// NAS にまだ無い。
    NoSidecar,
    /// 見た版の上で、端末が変わった。
    LocalChanged,
    /// NAS の版が未着手（端末は着手済み）。**取り込まずに書く**（今回の現象への対策）。
    TheirsUntouched,
    /// 星とセッションは同じで、手直し・境目が端末にだけある。
    MineHasMore,
}

/// 取り込むときの理由。どれも確認は要らない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PullReason {
    /// 端末が未着手。
    LocalUntouched,
    /// 早送り（NAS の版は、端末が見た版から続けて書かれたもの。端末は変わっていない）。
    FastForward,
    /// 星とセッションは同じで、手直し・境目が NAS にだけある。
    TheirsHasMore,
}

/// 確認するときの理由（ダイアログの理由の 1 行に使う）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ClashReason {
    /// どちらでも選別が進んでいて、中身が違う。
    Diverged,
    /// NAS の側で最初からやり直されている。
    TheirsRestarted,
    /// この端末でやり直したあと、NAS の側が進んでいる。
    MineRestarted,
    /// 星とセッションは同じで、手直し・境目が食い違う（両方にあって違う）。
    ExtrasConflict,
}

/// 進み具合の比べ（`a` から見た `b`）。表示用。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ProgressOrder {
    Ahead,
    Behind,
    Even,
    /// どちらが進んでいるとも言えない（ROUND と★で勝ち負けが食い違う、など）。
    Unclear,
}

/// 混ぜ方（ダイアログの D・E）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MergeMode {
    /// D「両方で残した写真のみにする」（積集合）。
    Intersection,
    /// E「どちらかで残した写真をすべて残す」（和集合）。
    Union,
}

/// D・E の見込み（ダイアログの 1 行）。
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MergePreview {
    pub mine_starred: u32,
    pub theirs_starred: u32,
    pub intersection_starred: u32,
    pub union_starred: u32,
    /// どちらかの途中の ROUND で、まだ見ていない写真の数（混ぜると今の★のままになる）。
    pub undecided: u32,
    /// どちらかが ROUND の途中か。
    pub mid_round: bool,
}

/// 開き方の判断（設計書 §4.3）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SidecarPlan {
    /// 何もしない。`seen` が Some なら、端末の控えをその値に進める（意味が同じだった）。
    Settled { seen: Option<SeenRecord>, reason: SettledReason },
    /// 端末の分を書く。`expected` は書く直前に NAS がこの版のままであること
    /// （楽観ロック。違えば書かずに判定し直す）。`aside_theirs` なら、NAS の版を
    /// `catalog.<相手>.json` に退避してから書く。
    Push { expected: Option<String>, aside_theirs: bool, reason: PushReason },
    /// NAS の版を確認なしに取り込む。`aside_mine` なら端末の分を退避してから。
    /// 取り込んだら端末の控えを `seen` にする。
    Pull { theirs: Sidecar, aside_mine: bool, seen: SeenRecord, reason: PullReason },
    /// 両方の要約を出して人に選ばせる（5 択。設計書 §4.6）。
    Clash {
        theirs: Sidecar,
        mine_progress: SidecarProgress,
        theirs_progress: SidecarProgress,
        /// この端末から見た NAS の版の進み具合。
        order: ProgressOrder,
        reason: ClashReason,
        preview: MergePreview,
    },
}

/// D・E で混ぜた結果。端末をこの状態にしてから、NAS にも書く（両方を同じにする）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MergeResult {
    /// 「混ぜた星の完了状態」のセッション（1 つ戻すはできない）。
    pub session: Session,
    /// 混ぜた★（★1 以上だけ。載っていない写真は★0）。
    pub ratings: HashMap<String, i32>,
    pub overrides: Vec<PairOverride>,
    pub burst_distance: Option<u32>,
    pub epoch: Option<String>,
    pub starred: u32,
    /// 途中の ROUND で、まだ見ていなかった写真の数。
    pub undecided: u32,
}

/// サイドカーの鍵と端末の写真の一致。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct KeyCoverage {
    pub matched: u32,
    pub total: u32,
}

// ---------------------------------------------------------------------------
// 正規化と比較（§4.2.1）
// ---------------------------------------------------------------------------

/// 写真の鍵をそろえる: 区切りを `/` に、Unicode を NFC に。
pub fn normalize_key(key: String) -> String {
    norm(&key)
}

fn norm(key: &str) -> String {
    key.replace('\\', "/").nfc().collect()
}

fn sorted_set<'a>(items: impl IntoIterator<Item = &'a String>) -> Vec<String> {
    items.into_iter().map(|item| norm(item)).collect::<BTreeSet<_>>().into_iter().collect()
}

fn canonical_session(session: &Session) -> JudgementSession {
    let mut members: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (representative, mates) in &session.members {
        let mates: BTreeSet<String> = mates.iter().map(|mate| norm(mate)).collect();
        // 仲間が 1 枚だけのものは持たない（start_round と同じ。持たないものは食い違わない）。
        if mates.len() > 1 {
            members.insert(norm(representative), mates);
        }
    }
    JudgementSession {
        round: session.round,
        target_star: session.target_star,
        finished: session.finished,
        remaining: session.current.iter().chain(&session.queue).map(|id| norm(id)).collect(),
        survivors: sorted_set(&session.survivors),
        members: members
            .into_iter()
            .map(|(representative, mates)| MemberGroup { representative, members: mates.into_iter().collect() })
            .collect(),
        history: session
            .history
            .iter()
            .map(|decision| JudgementDecision {
                group: sorted_set(&decision.group),
                chosen: sorted_set(&decision.chosen),
                topped: decision.topped.as_ref().map(|top| norm(&top.path)),
            })
            .collect(),
    }
}

fn canonical_overrides(overrides: &[PairOverride]) -> Vec<PairOverride> {
    let mut pairs: BTreeMap<(String, String), String> = BTreeMap::new();
    for over in overrides {
        // 同じ 2 枚が重なっていたら先に出た方（core の group_bursts が使う方）を残す。
        pairs.entry((norm(&over.left), norm(&over.right))).or_insert_with(|| over.decision.clone());
    }
    pairs
        .into_iter()
        .map(|((left, right), decision)| PairOverride { left, right, decision })
        .collect()
}

/// 選別状況を正規形にする。
///
/// 星の出どころ: **Session があれば `session.ratings`、無ければ `photos`**（行の星）。
/// PC は `photos` に全部の写真を★0 も含めて書き、Android は書かない。その違いを消すため、
/// ★0 の写真は持たない。
pub fn canonical_judgement(
    session: Option<Session>,
    photos: HashMap<String, i32>,
    overrides: Vec<PairOverride>,
    burst_distance: Option<u32>,
    epoch: Option<String>,
) -> Judgement {
    let source = match &session {
        Some(session) => &session.ratings,
        None => &photos,
    };
    let mut stars: BTreeMap<String, i32> = BTreeMap::new();
    for (path, rating) in source {
        if *rating > 0 {
            let entry = stars.entry(norm(path)).or_insert(0);
            *entry = (*entry).max(*rating);
        }
    }
    // 記録の顔ぶれ（D3）。セッションの ratings と photos の両方の鍵。
    let known: BTreeSet<String> = session
        .iter()
        .flat_map(|session| session.ratings.keys())
        .chain(photos.keys())
        .map(|path| norm(path))
        .collect();
    Judgement {
        stars: stars.into_iter().map(|(path, rating)| PhotoStar { path, rating }).collect(),
        session: session.as_ref().map(canonical_session),
        overrides: canonical_overrides(&overrides),
        burst_distance,
        epoch: epoch.filter(|epoch| !epoch.is_empty()),
        known: Some(known.into_iter().collect()),
    }
}

/// サイドカーの選別状況の正規形。鍵は先に `sidecar_normalize_keys` でそろえておく。
pub fn sidecar_judgement(sidecar: Sidecar) -> Judgement {
    canonical_judgement(
        sidecar.sessions.tournament,
        sidecar.photos.into_iter().map(|(path, photo)| (path, photo.rating)).collect(),
        sidecar.burst_overrides,
        sidecar.burst_distance,
        sidecar.epoch,
    )
}

/// 意味として同じか（正規形どうしで比べる）。
pub fn judgement_equivalent(a: Judgement, b: Judgement) -> bool {
    a == b
}

/// 比較キー。正規形を決まった順の JSON にして SHA-256 を取ったもの（`j1:` ＋ 16 進 64 桁）。
pub fn judgement_key(judgement: Judgement) -> String {
    key_of(&judgement)
}

fn key_of(judgement: &Judgement) -> String {
    // 正規形は Vec と Option だけでできているので、JSON は入れた順に関係なく同じ文字列になる。
    let view = JudgementKeyView {
        stars: &judgement.stars,
        session: &judgement.session,
        overrides: &judgement.overrides,
        burst_distance: &judgement.burst_distance,
        epoch: &judgement.epoch,
    };
    let bytes = serde_json::to_vec(&view).unwrap_or_default();
    let digest = Sha256::digest(&bytes);
    let mut out = String::with_capacity(JUDGEMENT_KEY_PREFIX.len() + 64);
    out.push_str(JUDGEMENT_KEY_PREFIX);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

// ---------------------------------------------------------------------------
// 未着手（§4.2.2）と進み具合
// ---------------------------------------------------------------------------

/// 未着手か。次の 4 つが**全部**成り立つときだけ未着手:
/// 1. ★1 以上の写真が 1 枚も無い
/// 2. セッションが無いか、1 手も進んでいない（ROUND 1・★0 が対象・履歴が空・終わっていない）
/// 3. 手直しが無い
/// 4. 学習した境目が無い
///
/// 「1 組決めて 1 つ戻した」は何も残っていないので未着手。「ROUND 1 を全部落として完了」は
/// 着手済み（全部落としたのも判断）。手直し・境目だけがあるのも着手済み。
pub fn is_untouched(judgement: Judgement) -> bool {
    untouched(&judgement)
}

fn untouched(judgement: &Judgement) -> bool {
    judgement.stars.is_empty()
        && judgement.overrides.is_empty()
        && judgement.burst_distance.is_none()
        && judgement.session.as_ref().map_or(true, |session| {
            session.round == 1 && session.target_star == 0 && session.history.is_empty() && !session.finished
        })
}

fn remaining_count(session: &JudgementSession) -> u32 {
    let groups: HashMap<&str, usize> = session
        .members
        .iter()
        .map(|group| (group.representative.as_str(), group.members.len()))
        .collect();
    session
        .remaining
        .iter()
        .map(|id| groups.get(id.as_str()).copied().unwrap_or(1) as u32)
        .sum()
}

/// 選別状況の要約（ダイアログの 1 行・catalog.json の `progress`）。`total` は分からないので None。
pub fn judgement_progress(judgement: Judgement) -> SidecarProgress {
    progress_of(&judgement)
}

fn progress_of(judgement: &Judgement) -> SidecarProgress {
    let session = judgement.session.as_ref();
    SidecarProgress {
        started: !untouched(judgement),
        round: session.map_or(0, |s| s.round),
        finished: session.map_or(false, |s| s.finished),
        decided: session.map_or(0, |s| s.history.len() as u32),
        remaining: session.map_or(0, remaining_count),
        starred: judgement.stars.len() as u32,
        total: None,
        overrides: judgement.overrides.len() as u32,
        learned: judgement.burst_distance.is_some(),
    }
}

fn sign<T: Ord>(a: T, b: T) -> i8 {
    match a.cmp(&b) {
        Ordering::Greater => 1,
        Ordering::Less => -1,
        Ordering::Equal => 0,
    }
}

/// どちらが進んでいるか（`a` から見て）。**表示だけに使い、自動の判断には使わない。時刻は使わない。**
///
/// 1. ROUND（終わっていれば半歩進んだと見る）
/// 2. この ROUND で決めた組の数（ROUND が同じときだけ比べる）
/// 3. ★1 以上の数
///
/// 1〜3 で勝ち負けが食い違えば Unclear。1〜3 が全部同じなら、手直しの数・境目の学習で比べる。
pub fn progress_cmp(a: SidecarProgress, b: SidecarProgress) -> ProgressOrder {
    let stage = |p: &SidecarProgress| p.round * 2 + u32::from(p.finished);
    let by_round = sign(stage(&a), stage(&b));
    let by_decided = if by_round == 0 { sign(a.decided, b.decided) } else { 0 };
    let by_starred = sign(a.starred, b.starred);
    let signs: Vec<i8> = [by_round, by_decided, by_starred].into_iter().filter(|s| *s != 0).collect();
    if signs.iter().any(|s| *s > 0) && signs.iter().any(|s| *s < 0) {
        return ProgressOrder::Unclear;
    }
    let decided = match signs.first() {
        Some(first) => *first,
        None => sign((a.overrides, a.learned), (b.overrides, b.learned)),
    };
    match decided {
        1 => ProgressOrder::Ahead,
        -1 => ProgressOrder::Behind,
        _ => ProgressOrder::Even,
    }
}

// ---------------------------------------------------------------------------
// 版の見分け
// ---------------------------------------------------------------------------

/// 版の見分け。`writeId` があればそれ、無ければ `legacy:<updatedAt>:<updatedBy>`
/// （古い版との互換。今の seenAt/seenBy からも同じ値を作れる）。
pub fn sidecar_token(sidecar: Sidecar) -> String {
    token_of(&sidecar)
}

fn token_of(sidecar: &Sidecar) -> String {
    match sidecar.write_id.as_deref() {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => format!("legacy:{}:{}", sidecar.updated_at, sidecar.updated_by),
    }
}

/// その版を「見た」ときの控え（取り込んだあと・意味が同じだったときに端末が控える値）。
pub fn sidecar_seen(sidecar: Sidecar) -> SeenRecord {
    let token = token_of(&sidecar);
    let epoch = sidecar.epoch.clone().filter(|epoch| !epoch.is_empty());
    SeenRecord { token, key: key_of(&sidecar_judgement(sidecar)), epoch }
}

/// 書く直前に、v2 の印を入れる: `version`・`writeId`（呼ぶ側が作った乱数）・
/// `basedOn`／`lineage`（いま NAS にある、置き換える版。無ければ None）・`keyBase`・`progress`。
///
/// 鍵は先にフォルダ形式（`sidecar_keys_to_folder`）にしておく。
pub fn sidecar_stamp(sidecar: Sidecar, write_id: String, base: Option<Sidecar>) -> Sidecar {
    let mut out = sidecar;
    out.version = SIDECAR_VERSION;
    out.write_id = Some(write_id);
    match base {
        Some(base) => {
            let token = token_of(&base);
            let mut lineage = vec![token.clone()];
            lineage.extend(base.lineage.unwrap_or_default().into_iter().filter(|id| *id != token));
            lineage.truncate(LINEAGE_LIMIT);
            out.based_on = Some(token);
            out.lineage = Some(lineage);
        }
        None => {
            out.based_on = None;
            out.lineage = None;
        }
    }
    out.key_base = Some(KEY_BASE_FOLDER.into());
    let total = out
        .photos
        .len()
        .max(out.sessions.tournament.as_ref().map_or(0, |session| session.ratings.len()));
    let mut progress = progress_of(&sidecar_judgement(out.clone()));
    progress.total = Some(total as u32);
    out.progress = Some(progress);
    out
}

// ---------------------------------------------------------------------------
// 開き方の判断（§4.3）
// ---------------------------------------------------------------------------

enum Extras {
    MineMore,
    TheirsMore,
    Conflict,
}

/// 星とセッションが同じときの、手直し・境目の違い方。
fn extras_relation(mine: &Judgement, theirs: &Judgement) -> Extras {
    let pairs = |judgement: &Judgement| -> BTreeMap<(String, String), String> {
        judgement
            .overrides
            .iter()
            .map(|over| ((over.left.clone(), over.right.clone()), over.decision.clone()))
            .collect()
    };
    let (mine_pairs, theirs_pairs) = (pairs(mine), pairs(theirs));
    let conflict = mine_pairs
        .iter()
        .any(|(pair, decision)| theirs_pairs.get(pair).is_some_and(|other| other != decision))
        || matches!((mine.burst_distance, theirs.burst_distance), (Some(a), Some(b)) if a != b);
    let mine_more = mine_pairs.keys().any(|pair| !theirs_pairs.contains_key(pair))
        || (mine.burst_distance.is_some() && theirs.burst_distance.is_none());
    let theirs_more = theirs_pairs.keys().any(|pair| !mine_pairs.contains_key(pair))
        || (theirs.burst_distance.is_some() && mine.burst_distance.is_none());
    match (conflict, mine_more, theirs_more) {
        (false, true, false) => Extras::MineMore,
        (false, false, true) => Extras::TheirsMore,
        // 両方に足りない分がある・食い違う → 確認（仮置き: 和集合にせず確認する）。
        _ => Extras::Conflict,
    }
}

fn total_of(sidecar: &Sidecar) -> u32 {
    sidecar
        .photos
        .len()
        .max(sidecar.sessions.tournament.as_ref().map_or(0, |session| session.ratings.len())) as u32
}

/// 開いたとき（と、書く直前）にどうするか。**時刻の大小では決めない。**
///
/// - `seen`: 端末の控え（最後に読んだ／書いた版）。一度も見ていなければ `SeenRecord::default()`。
/// - `local`: 端末の今の選別状況の正規形（鍵はフォルダ形式）。
/// - `remote`: 読めた NAS の版（鍵は `sidecar_normalize_keys` でそろえたもの）。まだ無ければ None。
///   **つながらない・壊れているときは呼ばない**（それは各環境が読む時点で弾く）。
/// - `writable`: 書ける共有か。
/// - `detached`: 「この端末の状況を残す」を選んだあとか（自動では書かない）。
///
/// 上から順に当てはめる（設計書 §4.3 の表）:
/// 1. NAS に無い → 未着手なら何もしない、着手済みなら書く
/// 2. 意味が同じ → 何もしない（警告なし。控えだけ進める）
/// 3. 見た版のまま → 端末が変わっていれば書く
/// 4. 端末が未着手 → 確認なしに取り込む
/// 5. NAS が未着手（端末は着手済み）→ 確認なしに書く（NAS の版は退避）
/// 6. 早送り（NAS の版の系統に、端末が見た版がある。端末は変わっていない）→ 確認なしに取り込む。
///    ただし NAS の側がやり直した版（epoch が違う）は早送りにせず、#9 で確認する（U42）
/// 7. 星とセッションが同じで、手直し・境目が片方にだけある → 持っている方に合わせる
/// 8. 書けない共有 → 何もしない（この端末だけの結果）
/// 9. やり直しが絡む → 確認
/// 10. それ以外（両方着手済みで違う）→ 確認
pub fn sidecar_plan(
    seen: SeenRecord,
    local: Judgement,
    remote: Option<Sidecar>,
    writable: bool,
    detached: bool,
) -> SidecarPlan {
    let settled = |reason| SidecarPlan::Settled { seen: None, reason };

    // 1. NAS に無い。
    let Some(theirs) = remote else {
        return if untouched(&local) {
            settled(SettledReason::Nothing)
        } else if !writable {
            settled(SettledReason::ReadOnly)
        } else if detached {
            settled(SettledReason::Detached)
        } else {
            SidecarPlan::Push { expected: None, aside_theirs: false, reason: PushReason::NoSidecar }
        };
    };

    let newer = theirs.version > SIDECAR_VERSION;
    let can_write = writable && !newer;
    let blocked = if newer { SettledReason::NewerVersion } else { SettledReason::ReadOnly };
    let token = token_of(&theirs);
    let remote_judgement = sidecar_judgement(theirs.clone());
    let remote_key = key_of(&remote_judgement);
    let local_key = key_of(&local);

    // 2. 意味が同じ。文字列・版が違っても何もしない。
    if local == remote_judgement {
        return SidecarPlan::Settled {
            seen: Some(SeenRecord { token, key: remote_key, epoch: remote_judgement.epoch.clone() }),
            reason: SettledReason::Same,
        };
    }

    let never_seen = seen.token.is_empty();
    // 端末が変わったか: 見た版の比較キーと今の比較キーが違うか（印ではなく中身で決める）。
    let local_changed = seen.key.is_empty() || local_key != seen.key;

    // 3. 見た版のまま。
    if !never_seen && token == seen.token {
        if !local_changed {
            return settled(SettledReason::NoChange);
        }
        if !can_write {
            return settled(blocked);
        }
        if detached {
            return settled(SettledReason::Detached);
        }
        // 着手済みの版を未着手（やり直した）で置き換えるときは、念のため退避する。
        let aside_theirs = untouched(&local) && !untouched(&remote_judgement);
        return SidecarPlan::Push { expected: Some(token), aside_theirs, reason: PushReason::LocalChanged };
    }

    // やり直しの検出（epoch）。見た版の世代から、どちらが変えたか。
    let remote_restarted =
        !never_seen && remote_judgement.epoch != seen.epoch && remote_judgement.epoch != local.epoch;
    let local_restarted = !never_seen && local.epoch != seen.epoch && local.epoch != remote_judgement.epoch;
    let local_untouched = untouched(&local) && !local_restarted;
    let remote_untouched = untouched(&remote_judgement) && !remote_restarted;

    let pull = |aside_mine: bool, reason: PullReason| SidecarPlan::Pull {
        theirs: theirs.clone(),
        aside_mine,
        seen: SeenRecord { token: token.clone(), key: remote_key.clone(), epoch: remote_judgement.epoch.clone() },
        reason,
    };
    let clash = |reason: ClashReason| {
        let mine_progress = progress_of(&local);
        let mut theirs_progress = progress_of(&remote_judgement);
        theirs_progress.total = Some(total_of(&theirs));
        SidecarPlan::Clash {
            theirs: theirs.clone(),
            order: progress_cmp(mine_progress.clone(), theirs_progress.clone()),
            mine_progress,
            theirs_progress,
            reason,
            preview: preview_of(&local, &remote_judgement),
        }
    };

    // 4. 端末が未着手 → 取り込む（捨てるものが無い）。
    if local_untouched {
        return pull(false, PullReason::LocalUntouched);
    }

    // 5. NAS が未着手 → 端末の分を書く。取り込まない（今回の現象）。
    if remote_untouched {
        if !can_write {
            return settled(blocked);
        }
        return SidecarPlan::Push { expected: Some(token), aside_theirs: true, reason: PushReason::TheirsUntouched };
    }

    // 6. 早送り。系統（basedOn・lineage）に端末が見た版がある。古い形（writeId 無し）は見なさない。
    //    **NAS の側がやり直した版（epoch が違う）は早送りにしない**（U42。ユーザー決定 2026-10-02）。
    //    端末が着手済みなら #9 で確認する（未着手なら #4 で取り込み済み、同じなら #2 で済み）。
    let descends = theirs.write_id.as_deref().is_some_and(|id| !id.is_empty())
        && (theirs.based_on.as_deref() == Some(seen.token.as_str())
            || theirs.lineage.as_ref().is_some_and(|lineage| lineage.iter().any(|id| *id == seen.token)));
    if !never_seen && !local_changed && !remote_restarted && descends {
        return pull(true, PullReason::FastForward);
    }

    // 7. 手直し・境目だけが違う。
    if local.stars == remote_judgement.stars
        && local.session == remote_judgement.session
        && local.epoch == remote_judgement.epoch
    {
        match extras_relation(&local, &remote_judgement) {
            Extras::TheirsMore => return pull(false, PullReason::TheirsHasMore),
            Extras::MineMore => {
                if !can_write {
                    return settled(blocked);
                }
                if detached {
                    return settled(SettledReason::Detached);
                }
                return SidecarPlan::Push {
                    expected: Some(token),
                    aside_theirs: false,
                    reason: PushReason::MineHasMore,
                };
            }
            Extras::Conflict => return clash(ClashReason::ExtrasConflict),
        }
    }

    // 8. 書けない共有。端末の分はそのまま。
    if !can_write {
        return settled(blocked);
    }

    // 9. やり直しが絡む。
    if remote_restarted {
        return clash(ClashReason::TheirsRestarted);
    }
    if local_restarted {
        return clash(ClashReason::MineRestarted);
    }

    // 10. 両方着手済みで違う。
    clash(ClashReason::Diverged)
}

// ---------------------------------------------------------------------------
// 混ぜ方（D・E。§4.6.2・§4.6.3）
// ---------------------------------------------------------------------------

/// その端末で「まだ見ていない」写真（途中のセッションの `remaining` と、その連写の仲間）。
/// セッションが無いか終わっていれば None。
fn pending_of(judgement: &Judgement) -> Option<HashSet<String>> {
    let session = judgement.session.as_ref().filter(|session| !session.finished)?;
    let groups: HashMap<&str, &Vec<String>> = session
        .members
        .iter()
        .map(|group| (group.representative.as_str(), &group.members))
        .collect();
    let mut pending = HashSet::new();
    for id in &session.remaining {
        pending.insert(id.clone());
        if let Some(mates) = groups.get(id.as_str()) {
            pending.extend(mates.iter().cloned());
        }
    }
    Some(pending)
}

/// 未判定か: 途中のセッションでまだ見ていない、セッションが無く★も 0、
/// または**その端末の記録に無い**（D3。PC だけが対象にする RAW・HEIC など）。
fn undecided(judgement: &Judgement, pending: &Option<HashSet<String>>, path: &str, star: i32) -> bool {
    let recorded = judgement
        .known
        .as_ref()
        .is_none_or(|known| known.binary_search_by(|id| id.as_str().cmp(path)).is_ok());
    match (&judgement.session, pending) {
        (None, _) => star == 0,
        (Some(_), Some(pending)) => !recorded || pending.contains(path),
        (Some(_), None) => !recorded,
    }
}

fn stars_map(judgement: &Judgement) -> BTreeMap<String, i32> {
    judgement.stars.iter().map(|star| (star.path.clone(), star.rating)).collect()
}

fn merged_stars(mine: &Judgement, theirs: &Judgement, mode: MergeMode) -> BTreeMap<String, i32> {
    let (mine_stars, theirs_stars) = (stars_map(mine), stars_map(theirs));
    let (mine_pending, theirs_pending) = (pending_of(mine), pending_of(theirs));
    let mut out = BTreeMap::new();
    // ★0 どうしは★0 にしかならないので、どちらかで★1 以上の写真だけ見ればよい。
    for path in mine_stars.keys().chain(theirs_stars.keys()) {
        if out.contains_key(path) {
            continue;
        }
        let a = mine_stars.get(path).copied().unwrap_or(0);
        let b = theirs_stars.get(path).copied().unwrap_or(0);
        let star = match mode {
            MergeMode::Union => a.max(b),
            MergeMode::Intersection => {
                let a_open = undecided(mine, &mine_pending, path, a);
                let b_open = undecided(theirs, &theirs_pending, path, b);
                match (a_open, b_open) {
                    // 片方が未判定 → 判定済みの側の★を採る（§6 Q5 の案 1）。
                    (true, false) => b,
                    (false, true) => a,
                    _ => a.min(b),
                }
            }
        };
        if star > 0 {
            out.insert(path.clone(), star);
        }
    }
    out
}

/// 星を写真ごとに混ぜる。和集合は大きい方、積集合は小さい方（片方が未判定なら判定済みの側）。
pub fn merge_stars(mine: Judgement, theirs: Judgement, mode: MergeMode) -> HashMap<String, i32> {
    merged_stars(&mine, &theirs, mode).into_iter().collect()
}

/// 手直しを和集合にする。同じ 2 枚で食い違ったら**この端末の方**を採る（§6 Q7）。
pub fn merge_overrides(mine: Vec<PairOverride>, theirs: Vec<PairOverride>) -> Vec<PairOverride> {
    let mut all = canonical_overrides(&mine);
    all.extend(canonical_overrides(&theirs));
    // canonical_overrides は先に出た方を残すので、端末の分が勝つ。
    canonical_overrides(&all)
}

/// 混ぜた星から「完了した状態」のセッションを作る（§4.6.3）。
///
/// `finished = true`・`queue`／`current`／`history` は空（1 つ戻すはできない）。
/// `survivors` は★が `target_star` を超え、★5 未満の写真（★5 は以降のラウンドに出さない）。
/// 続きは結果画面の「もう一度選別する（★n から）」（`round_for`）で始める。
pub fn session_from_ratings(
    ratings: HashMap<String, i32>,
    round: u32,
    target_star: i32,
    group_size: u32,
    members: HashMap<String, Vec<String>>,
) -> Session {
    let survivors: Vec<String> = ratings
        .iter()
        .filter(|(_, star)| **star > target_star && **star < MAX_STAR)
        .map(|(path, _)| path.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    Session {
        group_size: group_size.max(1),
        target_star,
        round: round.max(1),
        queue: Vec::new(),
        current: Vec::new(),
        survivors,
        ratings,
        members,
        history: Vec::new(),
        finished: true,
    }
}

fn preview_of(mine: &Judgement, theirs: &Judgement) -> MergePreview {
    let (mine_pending, theirs_pending) = (pending_of(mine), pending_of(theirs));
    let mut open: HashSet<String> = HashSet::new();
    open.extend(mine_pending.iter().flatten().cloned());
    open.extend(theirs_pending.iter().flatten().cloned());
    MergePreview {
        mine_starred: mine.stars.len() as u32,
        theirs_starred: theirs.stars.len() as u32,
        intersection_starred: merged_stars(mine, theirs, MergeMode::Intersection).len() as u32,
        union_starred: merged_stars(mine, theirs, MergeMode::Union).len() as u32,
        undecided: open.len() as u32,
        mid_round: mine_pending.is_some() || theirs_pending.is_some(),
    }
}

/// D・E の見込み（ダイアログの 1 行）。
pub fn merge_preview(mine: Judgement, theirs: Judgement) -> MergePreview {
    preview_of(&mine, &theirs)
}

/// D・E: 両方の選別状況から新しい 1 つを作る。
///
/// - 星: `merge_stars`
/// - セッション: 混ぜられないので「混ぜた星の完了状態」（`session_from_ratings`）。ROUND と
///   対象の★は両方の大きい方。連写のまとまりは端末の分（無ければ NAS の分）
/// - 手直し: 和集合（食い違えば端末）。境目: 端末にあれば端末、無ければ NAS
/// - やり直しの世代: 同じならそのまま、違えば `fresh_epoch`（呼ぶ側が作った乱数）
pub fn merge_judgements(
    mine: Judgement,
    theirs: Judgement,
    mode: MergeMode,
    group_size: u32,
    fresh_epoch: String,
) -> MergeResult {
    let ratings: HashMap<String, i32> = merged_stars(&mine, &theirs, mode).into_iter().collect();
    let sessions = || mine.session.iter().chain(theirs.session.iter());
    let round = sessions().map(|session| session.round).max().unwrap_or(1);
    let target_star = sessions().map(|session| session.target_star).max().unwrap_or(0);
    let members: HashMap<String, Vec<String>> = mine
        .session
        .as_ref()
        .or(theirs.session.as_ref())
        .map(|session| {
            session
                .members
                .iter()
                .map(|group| (group.representative.clone(), group.members.clone()))
                .collect()
        })
        .unwrap_or_default();
    let undecided = preview_of(&mine, &theirs).undecided;
    let starred = ratings.len() as u32;
    MergeResult {
        session: session_from_ratings(ratings.clone(), round, target_star, group_size, members),
        ratings,
        overrides: merge_overrides(mine.overrides.clone(), theirs.overrides.clone()),
        burst_distance: mine.burst_distance.or(theirs.burst_distance),
        epoch: if mine.epoch == theirs.epoch { mine.epoch.clone() } else { Some(fresh_epoch) },
        starred,
        undecided,
    }
}

// ---------------------------------------------------------------------------
// 写真の鍵（§4.8）
// ---------------------------------------------------------------------------

fn map_session(session: Session, f: &dyn Fn(&str) -> String) -> Session {
    let list = |items: Vec<String>| items.iter().map(|item| f(item)).collect::<Vec<_>>();
    let table = |items: HashMap<String, i32>| items.into_iter().map(|(key, value)| (f(&key), value)).collect();
    Session {
        group_size: session.group_size,
        target_star: session.target_star,
        round: session.round,
        queue: list(session.queue),
        current: list(session.current),
        survivors: list(session.survivors),
        ratings: table(session.ratings),
        members: session
            .members
            .into_iter()
            .map(|(representative, mates)| (f(&representative), list(mates)))
            .collect(),
        history: session
            .history
            .into_iter()
            .map(|decision| Decision {
                group: list(decision.group),
                chosen: list(decision.chosen),
                topped: decision.topped.map(|top| Topped { path: f(&top.path), previous: top.previous }),
                before: table(decision.before),
            })
            .collect(),
        finished: session.finished,
    }
}

fn map_sidecar(sidecar: Sidecar, f: &dyn Fn(&str) -> String) -> Sidecar {
    let mut out = sidecar;
    out.photos = std::mem::take(&mut out.photos).into_iter().map(|(key, photo)| (f(&key), photo)).collect();
    out.burst_overrides = std::mem::take(&mut out.burst_overrides)
        .into_iter()
        .map(|over| PairOverride { left: f(&over.left), right: f(&over.right), decision: over.decision })
        .collect();
    out.sessions.tournament = out.sessions.tournament.take().map(|session| map_session(session, f));
    out
}

fn trimmed_prefix(prefix: &str) -> String {
    norm(prefix).trim_matches('/').to_string()
}

/// 端末の中の鍵 → サイドカーの鍵（選んだフォルダからの相対・`/`・NFC）。書く前に通す。
///
/// `prefix` は鍵の頭から外すもの（Android は共有の根からのフォルダのパス。PC は空）。
/// Session の中の鍵（ratings・queue・current・survivors・members・history）もすべて変える。
pub fn sidecar_keys_to_folder(sidecar: Sidecar, prefix: String) -> Sidecar {
    let head = trimmed_prefix(&prefix);
    let strip = move |key: &str| -> String {
        let key = norm(key);
        if !head.is_empty() {
            if let Some(rest) = key.strip_prefix(&format!("{head}/")) {
                return rest.to_string();
            }
        }
        key
    };
    let mut out = map_sidecar(sidecar, &strip);
    out.key_base = Some(KEY_BASE_FOLDER.into());
    out
}

/// サイドカーの鍵（フォルダ形式）→ 端末の中の鍵。取り込む前に通す。
///
/// `prefix` を頭に付け（Android はフォルダのパス、PC は空）、区切りを `separator` にする
/// （PC の Windows は `\`、ほかは `/`）。`keyBase` は外す（端末の形なので）。
pub fn sidecar_keys_from_folder(sidecar: Sidecar, prefix: String, separator: String) -> Sidecar {
    let head = trimmed_prefix(&prefix);
    let separator = if separator.is_empty() { "/".to_string() } else { separator };
    let add = move |key: &str| -> String {
        let joined = if head.is_empty() { key.to_string() } else { format!("{head}/{key}") };
        if separator == "/" {
            joined
        } else {
            joined.replace('/', &separator)
        }
    };
    let mut out = map_sidecar(sidecar, &add);
    out.key_base = None;
    out
}

fn all_keys(sidecar: &Sidecar) -> Vec<String> {
    let mut keys: Vec<String> = sidecar.photos.keys().map(|key| norm(key)).collect();
    for over in &sidecar.burst_overrides {
        keys.push(norm(&over.left));
        keys.push(norm(&over.right));
    }
    if let Some(session) = &sidecar.sessions.tournament {
        keys.extend(session.ratings.keys().map(|key| norm(key)));
        keys.extend(session.current.iter().chain(&session.queue).chain(&session.survivors).map(|key| norm(key)));
    }
    keys
}

/// 読んだ直後に、鍵をフォルダ形式にそろえる。
///
/// `keyBase` が `"folder"` なら区切りと NFC だけ。無ければ（古い版）、**鍵が全部同じ頭の
/// フォルダで始まっていて、その頭が `folder_hint`（この端末で選んだフォルダのパス）の末尾と
/// 一致すれば**、古い Android の形（共有の根からの相対）と見なして外す。合わなければ外さない。
pub fn sidecar_normalize_keys(sidecar: Sidecar, folder_hint: String) -> Sidecar {
    if sidecar.key_base.as_deref() == Some(KEY_BASE_FOLDER) {
        return map_sidecar(sidecar, &norm);
    }
    let keys = all_keys(&sidecar);
    let mut common: Option<Vec<String>> = None;
    for key in &keys {
        let mut parts: Vec<String> = key.split('/').map(str::to_string).collect();
        parts.pop(); // ファイル名
        common = Some(match common {
            None => parts,
            Some(previous) => previous.into_iter().zip(parts).take_while(|(a, b)| a == b).map(|(a, _)| a).collect(),
        });
    }
    let common = common.unwrap_or_default();
    let hint: Vec<String> = folder_hint
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .map(|part| part.nfc().collect())
        .collect();
    let mut strip = 0;
    for length in (1..=common.len().min(hint.len())).rev() {
        if common[..length] == hint[hint.len() - length..] {
            strip = length;
            break;
        }
    }
    sidecar_keys_to_folder(sidecar, common[..strip].join("/"))
}

/// サイドカーの鍵のうち、端末の写真（`photo_keys`。フォルダ形式）にあるものの数。
/// 取り込む前に確かめる（一致が半分未満なら、場所の違う記録の見込みが高い）。
pub fn sidecar_key_coverage(sidecar: Sidecar, photo_keys: Vec<String>) -> KeyCoverage {
    let photos: HashSet<String> = photo_keys.iter().map(|key| norm(key)).collect();
    let keys: BTreeSet<String> = all_keys(&sidecar)
        .into_iter()
        .filter(|key| !key.is_empty())
        .collect();
    KeyCoverage {
        matched: keys.iter().filter(|key| photos.contains(*key)).count() as u32,
        total: keys.len() as u32,
    }
}

#[cfg(test)]
mod tests;
