//! サイドカー同期の判断のテスト（U33）。
//!
//! 設計書 §3 の「危険なシナリオ」の表と §4.3 の判断の表を、1 行ずつ確かめる。
//! 行の番号は、関数名の末尾の「§3-n」「#n」で示す。

use super::*;
use crate::{
    advance, keep_top, resize, round_for, sidecar_decide, sidecar_from_json, sidecar_to_json, start_round, undo,
    BurstThreshold, PhotoRef, SidecarPhoto, SidecarSessions, SidecarSync,
};

const NAMES: [&str; 6] = ["a.jpg", "b.jpg", "sub/c.jpg", "d.jpg", "e.jpg", "f.jpg"];

fn thr() -> BurstThreshold {
    BurstThreshold { window_ms: 4000, distance: 6, d_hash_version: 2 }
}

fn refs(names: &[&str]) -> Vec<PhotoRef> {
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

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|name| name.to_string()).collect()
}

/// 「選別を開始」を押しただけ（未着手）。
fn fresh() -> Session {
    start_round(refs(&NAMES), 2, 0, false, thr(), vec![])
}

/// ROUND 1 で 2 組決めた（★1 が a・sub/c・d）。
fn progressed() -> Session {
    let session = advance(fresh(), names(&["a.jpg"]));
    advance(session, names(&["sub/c.jpg", "d.jpg"]))
}

/// 別の端末で 1 組決めた（★1 が b）。
fn other_progress() -> Session {
    advance(fresh(), names(&["b.jpg"]))
}

fn judge(session: Option<Session>) -> Judgement {
    canonical_judgement(session, HashMap::new(), vec![], None, None)
}

fn sidecar(session: Option<Session>, write_id: Option<&str>, by: &str, at: i64) -> Sidecar {
    Sidecar {
        version: 1,
        updated_at: at,
        updated_by: by.into(),
        updated_by_name: by.into(),
        photos: HashMap::new(),
        burst_overrides: vec![],
        sessions: SidecarSessions { tournament: session },
        burst_distance: None,
        write_id: write_id.map(Into::into),
        based_on: None,
        lineage: None,
        epoch: None,
        key_base: Some(KEY_BASE_FOLDER.into()),
        progress: None,
        settings: None,
    }
}

fn seen_of(sidecar: &Sidecar) -> SeenRecord {
    sidecar_seen(sidecar.clone())
}

fn split(left: &str, right: &str) -> PairOverride {
    PairOverride { left: left.into(), right: right.into(), decision: "split".into() }
}

fn join(left: &str, right: &str) -> PairOverride {
    PairOverride { left: left.into(), right: right.into(), decision: "join".into() }
}

fn stars(list: &[(&str, i32)]) -> HashMap<String, i32> {
    list.iter().map(|(name, star)| (name.to_string(), *star)).collect()
}

// ---------------------------------------------------------------------------
// 正規化と比較（§4.2.1）
// ---------------------------------------------------------------------------

#[test]
fn 正規化_pc形式とandroid形式の同じ選別状況は同じ() {
    let session = progressed();

    // Android: Session だけ・旧い l/r/d の手直し。
    let mut android = sidecar(Some(session.clone()), None, "android", 1);
    android.burst_overrides = vec![split("a.jpg", "b.jpg")];
    android.burst_distance = Some(9);
    let android_json = sidecar_to_json(android)
        .replace("\"left\"", "\"l\"")
        .replace("\"right\"", "\"r\"")
        .replace("\"decision\"", "\"d\"");
    assert!(android_json.contains("\"l\""));
    let android = sidecar_from_json(android_json).expect("読めるはず");

    // PC: photos に全部の写真を 0 も含めて載せる・区切りは `\`・別の時刻と端末。
    let mut pc = sidecar(Some(session.clone()), Some("w-pc"), "pc", 999);
    for name in NAMES {
        let rating = session.ratings.get(name).copied().unwrap_or(0);
        pc.photos.insert(name.into(), SidecarPhoto { rating });
    }
    pc.burst_overrides = vec![split("a.jpg", "b.jpg")];
    pc.burst_distance = Some(9);
    let pc = sidecar_keys_from_folder(pc, String::new(), "\\".into());
    assert!(pc.photos.contains_key("sub\\c.jpg"));

    assert!(judgement_equivalent(sidecar_judgement(android.clone()), sidecar_judgement(pc.clone())));
    assert_eq!(judgement_key(sidecar_judgement(android)), judgement_key(sidecar_judgement(pc)));
}

#[test]
fn 正規化_group_sizeだけ違えば同じ() {
    // 1 組決めたところ: current [sub/c, d]・queue [e, f]。4 枚にすると current に全部入る。
    let session = advance(fresh(), names(&["a.jpg"]));
    let wider = resize(session.clone(), 4);
    assert_ne!(session.current, wider.current);
    assert!(judgement_equivalent(judge(Some(session)), judge(Some(wider))));
}

#[test]
fn 正規化_historyのbeforeだけ違えば同じ() {
    let session = progressed();
    let mut other = session.clone();
    other.history[0].before.insert("zzz.jpg".into(), 4);
    assert!(judgement_equivalent(judge(Some(session)), judge(Some(other))));
}

#[test]
fn 正規化_jsonの並びと空白と更新の印だけ違えば同じで警告しない() {
    let original = sidecar(Some(progressed()), Some("w-1"), "android", 10);
    let seen = seen_of(&original);

    // 並び（serde_json::Value は鍵を並べ替える）・空白・updatedAt・updatedBy・writeId を変える。
    let value: serde_json::Value = serde_json::from_str(&sidecar_to_json(original.clone())).unwrap();
    let pretty = serde_json::to_string_pretty(&value)
        .unwrap()
        .replace("\"updatedAt\": 10", "\"updatedAt\": 77777")
        .replace("\"updatedBy\": \"android\"", "\"updatedBy\": \"pc\"")
        .replace("\"w-1\"", "\"w-2\"");
    assert_ne!(pretty, sidecar_to_json(original.clone()));
    let rewritten = sidecar_from_json(pretty).expect("読めるはず");
    assert_ne!(sidecar_token(rewritten.clone()), sidecar_token(original.clone()));

    assert!(judgement_equivalent(sidecar_judgement(original), sidecar_judgement(rewritten.clone())));
    match sidecar_plan(seen, judge(Some(progressed())), Some(rewritten), true, false) {
        SidecarPlan::Settled { seen: Some(next), reason: SettledReason::Same } => {
            assert_eq!(next.token, "w-2");
        }
        other => panic!("意味が同じなら何もしない（控えだけ進める）はず: {other:?}"),
    }
}

#[test]
fn 正規化_星が1つ違えば違う() {
    let session = progressed();
    let mut other = session.clone();
    other.ratings.insert("e.jpg".into(), 1);
    assert!(!judgement_equivalent(judge(Some(session.clone())), judge(Some(other.clone()))));
    assert_ne!(judgement_key(judge(Some(session))), judgement_key(judge(Some(other))));
}

#[test]
fn 正規化_星0と鍵が無いのは同じ() {
    let with_zero = canonical_judgement(None, stars(&[("a.jpg", 2), ("b.jpg", 0)]), vec![], None, None);
    let without = canonical_judgement(None, stars(&[("a.jpg", 2)]), vec![], None, None);
    assert!(judgement_equivalent(with_zero, without));
}

#[test]
fn 正規化_nfdとnfcの鍵は同じ() {
    // 「ガ」を NFD（カ＋濁点）と NFC で。
    let nfd = canonical_judgement(None, stars(&[("\u{30AB}\u{3099}.jpg", 1)]), vec![], None, None);
    let nfc = canonical_judgement(None, stars(&[("\u{30AC}.jpg", 1)]), vec![], None, None);
    assert!(judgement_equivalent(nfd, nfc));
}

#[test]
fn 正規化_手直しの書き方と並びと重複は比べない() {
    let a = canonical_judgement(
        None,
        HashMap::new(),
        vec![split("a.jpg", "b.jpg"), join("d.jpg", "e.jpg")],
        None,
        None,
    );
    // 並びが逆・同じ 2 枚の重複（core の group_bursts は先に出た方を使う）。
    let b = canonical_judgement(
        None,
        HashMap::new(),
        vec![join("d.jpg", "e.jpg"), split("a.jpg", "b.jpg"), join("a.jpg", "b.jpg")],
        None,
        None,
    );
    // b は d-e が先なので、a-b は split が先に出ている（join は後の重複で捨てる）。
    assert!(judgement_equivalent(a, b));
}

#[test]
fn 比較キー_hashmapの順に左右されない() {
    let mut forward = HashMap::new();
    let mut backward = HashMap::new();
    for index in 0..300 {
        forward.insert(format!("IMG_{index:04}.JPG"), (index % 6) as i32);
    }
    for index in (0..300).rev() {
        backward.insert(format!("IMG_{index:04}.JPG"), (index % 6) as i32);
    }
    let a = judgement_key(canonical_judgement(None, forward, vec![], Some(9), None));
    let b = judgement_key(canonical_judgement(None, backward, vec![], Some(9), None));
    assert_eq!(a, b);
    assert!(a.starts_with("j1:"));
    assert_eq!(a.len(), 3 + 64);
}

// ---------------------------------------------------------------------------
// 未着手（§4.2.2）
// ---------------------------------------------------------------------------

#[test]
fn 未着手_何も無い_開始を押しただけ() {
    assert!(is_untouched(Judgement::default()));
    assert!(is_untouched(judge(Some(fresh()))));
}

#[test]
fn 未着手_1組決めて1つ戻した() {
    let back = undo(advance(fresh(), names(&["a.jpg"])));
    assert!(is_untouched(judge(Some(back.clone()))));
    assert!(judgement_equivalent(judge(Some(back)), judge(Some(fresh()))));
}

#[test]
fn 着手済み_round1を全部落として完了() {
    let mut session = fresh();
    while !session.finished {
        session = advance(session, vec![]);
    }
    assert_eq!(session.round, 1);
    assert!(!is_untouched(judge(Some(session))));
}

#[test]
fn 着手済み_境目だけ学習_手直しだけ() {
    let learned = canonical_judgement(Some(fresh()), HashMap::new(), vec![], Some(9), None);
    assert!(!is_untouched(learned));
    let edited = canonical_judgement(Some(fresh()), HashMap::new(), vec![split("a.jpg", "b.jpg")], None, None);
    assert!(!is_untouched(edited));
}

#[test]
fn 着手済み_1組でも決めた_セッションが無くても星がある() {
    assert!(!is_untouched(judge(Some(other_progress()))));
    assert!(!is_untouched(canonical_judgement(None, stars(&[("a.jpg", 1)]), vec![], None, None)));
}

// ---------------------------------------------------------------------------
// 開き方の判断（§4.3）。危険なシナリオ（§3）を 1 行ずつ
// ---------------------------------------------------------------------------

#[test]
fn 計画_nasに無く端末が未着手なら何もしない_3の1() {
    match sidecar_plan(SeenRecord::default(), judge(Some(fresh())), None, true, false) {
        SidecarPlan::Settled { seen: None, reason: SettledReason::Nothing } => {}
        other => panic!("未着手は書かないはず: {other:?}"),
    }
}

#[test]
fn 計画_nasに無く端末が着手済みなら書く_1() {
    match sidecar_plan(SeenRecord::default(), judge(Some(progressed())), None, true, false) {
        SidecarPlan::Push { expected: None, aside_theirs: false, reason: PushReason::NoSidecar } => {}
        other => panic!("書くはず: {other:?}"),
    }
    // 書けない・切り離し中なら書かない。
    assert!(matches!(
        sidecar_plan(SeenRecord::default(), judge(Some(progressed())), None, false, false),
        SidecarPlan::Settled { reason: SettledReason::ReadOnly, .. }
    ));
    assert!(matches!(
        sidecar_plan(SeenRecord::default(), judge(Some(progressed())), None, true, true),
        SidecarPlan::Settled { reason: SettledReason::Detached, .. }
    ));
}

#[test]
fn 計画_両方未着手で同じなら何もしない_3の1() {
    let theirs = sidecar(Some(fresh()), Some("w-pc"), "pc", 5);
    match sidecar_plan(SeenRecord::default(), judge(Some(fresh())), Some(theirs), true, false) {
        SidecarPlan::Settled { seen: Some(_), reason: SettledReason::Same } => {}
        other => panic!("同じなら何もしない: {other:?}"),
    }
}

/// 今回の現象（§2.2・§3-2）: Android は進んでいて、NAS には PC が「開始」を
/// 押しただけの版がある。Android は自分が最後に書いた版から何も変えていない。
#[test]
fn 計画_端末が進んでいてnasが未着手の版なら書く_取り込まない_5_3の2() {
    let local = progressed();
    let a1 = sidecar(Some(local.clone()), Some("w-a1"), "android", 10);
    let seen = seen_of(&a1);
    let p1 = sidecar(Some(fresh()), Some("w-p1"), "pc", 20);

    // 今の判断（sidecar_decide）では取り込みになり、端末の選別状況が消える。
    assert!(matches!(sidecar_decide(10, "android".into(), false, Some(p1.clone())), SidecarSync::Pull(_)));

    match sidecar_plan(seen, judge(Some(local)), Some(p1), true, false) {
        SidecarPlan::Push { expected, aside_theirs, reason } => {
            assert_eq!(expected.as_deref(), Some("w-p1"));
            assert!(aside_theirs, "NAS の未着手の版は退避する");
            assert_eq!(reason, PushReason::TheirsUntouched);
        }
        other => panic!("端末の分を書くはず: {other:?}"),
    }
}

#[test]
fn 計画_nasが未着手の版でも古い形でも書く_5() {
    // PC の古い版（writeId 無し）が書いた未着手の版。
    let local = progressed();
    let a1 = sidecar(Some(local.clone()), Some("w-a1"), "android", 10);
    let mut p1 = sidecar(Some(fresh()), None, "pc", 20);
    p1.key_base = None;
    match sidecar_plan(seen_of(&a1), judge(Some(local)), Some(p1), true, false) {
        SidecarPlan::Push { expected, aside_theirs: true, .. } => {
            assert_eq!(expected.as_deref(), Some("legacy:20:pc"));
        }
        other => panic!("書くはず: {other:?}"),
    }
}

#[test]
fn 計画_端末が未着手でnasが着手済みなら確認なしに取り込む_4_3の2() {
    // PC で「開始」を押しただけ。NAS には Android の進んだ版。
    let theirs = sidecar(Some(progressed()), Some("w-a1"), "android", 10);
    match sidecar_plan(SeenRecord::default(), judge(Some(fresh())), Some(theirs), true, false) {
        SidecarPlan::Pull { aside_mine, seen, reason, .. } => {
            assert!(!aside_mine, "捨てるものが無い");
            assert_eq!(reason, PullReason::LocalUntouched);
            assert_eq!(seen.token, "w-a1");
        }
        other => panic!("取り込むはず: {other:?}"),
    }
}

#[test]
fn 計画_再インストール_端末が空なら取り込み_何か残っていれば確認_3の9() {
    let theirs = sidecar(Some(progressed()), Some("w-a1"), "android", 10);
    assert!(matches!(
        sidecar_plan(SeenRecord::default(), Judgement::default(), Some(theirs.clone()), true, false),
        SidecarPlan::Pull { reason: PullReason::LocalUntouched, .. }
    ));
    assert!(matches!(
        sidecar_plan(SeenRecord::default(), judge(Some(other_progress())), Some(theirs), true, false),
        SidecarPlan::Clash { reason: ClashReason::Diverged, .. }
    ));
}

#[test]
fn 計画_作り直したプロジェクトは前の記録を取り込む_3の10() {
    // 新しいプロジェクトは未着手。NAS に前のプロジェクトの着手済みの版が残っている。
    let old = sidecar(Some(progressed()), None, "android-old", 3);
    assert!(matches!(
        sidecar_plan(SeenRecord::default(), judge(Some(fresh())), Some(old), true, false),
        SidecarPlan::Pull { reason: PullReason::LocalUntouched, .. }
    ));
}

#[test]
fn 計画_両方着手で違えば端末に変更が無くても確認_10_3の3() {
    let local = progressed();
    let a1 = sidecar(Some(local.clone()), Some("w-a1"), "android", 10);
    // 別の端末（古い形。系統が分からない）が、別の判断で上書きした。
    let theirs = sidecar(Some(other_progress()), None, "pc", 20);

    // 今の判断では取り込みになる（端末の 2 組が消える）。
    assert!(matches!(sidecar_decide(10, "android".into(), false, Some(theirs.clone())), SidecarSync::Pull(_)));

    match sidecar_plan(seen_of(&a1), judge(Some(local)), Some(theirs), true, false) {
        SidecarPlan::Clash { mine_progress, theirs_progress, order, reason, preview, .. } => {
            assert_eq!(reason, ClashReason::Diverged);
            assert_eq!(mine_progress.decided, 2);
            assert_eq!(theirs_progress.decided, 1);
            assert_eq!(mine_progress.starred, 3);
            assert_eq!(theirs_progress.starred, 1);
            assert_eq!(mine_progress.round, 1);
            assert!(mine_progress.started && theirs_progress.started);
            assert_eq!(order, ProgressOrder::Ahead);
            assert_eq!(preview.mine_starred, 3);
            assert_eq!(preview.theirs_starred, 1);
            assert_eq!(preview.union_starred, 4);
        }
        other => panic!("確認するはず: {other:?}"),
    }
}

#[test]
fn 計画_早送りなら確認なしに取り込む_6() {
    let local = progressed();
    let a1 = sidecar(Some(local.clone()), Some("w-a1"), "android", 10);
    // PC が a1 を取り込んで続けた。
    let mut b1 = sidecar(Some(advance(progressed(), names(&["e.jpg"]))), Some("w-b1"), "pc", 20);
    b1.based_on = Some("w-a1".into());
    b1.lineage = Some(vec!["w-a1".into()]);
    match sidecar_plan(seen_of(&a1), judge(Some(local)), Some(b1), true, false) {
        SidecarPlan::Pull { aside_mine, seen, reason, .. } => {
            assert_eq!(reason, PullReason::FastForward);
            assert!(aside_mine, "端末に控えを 1 つ残す");
            assert_eq!(seen.token, "w-b1");
        }
        other => panic!("早送りで取り込むはず: {other:?}"),
    }
}

#[test]
fn 計画_何手先でも系統がつながっていれば早送り_6() {
    let local = progressed();
    let a1 = sidecar(Some(local.clone()), Some("w-a1"), "android", 10);
    // PC が 2 回書いた（ラウンドの終わりと、窓を隠したとき）。
    let mut b2 = sidecar(Some(advance(progressed(), names(&["e.jpg"]))), Some("w-b2"), "pc", 30);
    b2.based_on = Some("w-b1".into());
    b2.lineage = Some(vec!["w-b1".into(), "w-a1".into()]);
    assert!(matches!(
        sidecar_plan(seen_of(&a1), judge(Some(local)), Some(b2), true, false),
        SidecarPlan::Pull { reason: PullReason::FastForward, .. }
    ));
}

#[test]
fn 計画_早送りでも端末が変わっていれば確認_3の4() {
    let a1 = sidecar(Some(progressed()), Some("w-a1"), "android", 10);
    let mut b1 = sidecar(Some(advance(progressed(), names(&["e.jpg"]))), Some("w-b1"), "pc", 20);
    b1.based_on = Some("w-a1".into());
    b1.lineage = Some(vec!["w-a1".into()]);
    // 端末も a1 から続けた（別の判断）。
    let local = advance(progressed(), names(&["f.jpg"]));
    assert!(matches!(
        sidecar_plan(seen_of(&a1), judge(Some(local)), Some(b1), true, false),
        SidecarPlan::Clash { reason: ClashReason::Diverged, .. }
    ));
}

#[test]
fn 計画_古いアプリが書いた版は早送りと見なさない_3の16() {
    let local = progressed();
    let a1 = sidecar(Some(local.clone()), Some("w-a1"), "android", 10);
    // 古いアプリは writeId・basedOn を書かない（読んでも捨てる）。
    let legacy = sidecar(Some(advance(progressed(), names(&["e.jpg"]))), None, "old-pc", 20);
    assert!(matches!(
        sidecar_plan(seen_of(&a1), judge(Some(local)), Some(legacy), true, false),
        SidecarPlan::Clash { reason: ClashReason::Diverged, .. }
    ));
}

#[test]
fn 計画_見た版のままで端末が変わっていれば書く_変わっていなければ何もしない_3() {
    let a1 = sidecar(Some(progressed()), Some("w-a1"), "android", 10);
    let changed = advance(progressed(), names(&["e.jpg"]));
    match sidecar_plan(seen_of(&a1), judge(Some(changed)), Some(a1.clone()), true, false) {
        SidecarPlan::Push { expected, aside_theirs: false, reason: PushReason::LocalChanged } => {
            assert_eq!(expected.as_deref(), Some("w-a1"));
        }
        other => panic!("書くはず: {other:?}"),
    }
    assert!(matches!(
        sidecar_plan(seen_of(&a1), judge(Some(progressed())), Some(a1), true, false),
        SidecarPlan::Settled { .. }
    ));
}

#[test]
fn 計画_見た版の比較キーが空なら変更ありと見なす() {
    // 移行の間: 古い dirty が true だった端末は、比較キーを空にして渡す。
    let a1 = sidecar(Some(progressed()), Some("w-a1"), "android", 10);
    let seen = SeenRecord { token: "w-a1".into(), key: String::new(), epoch: None };
    let local = advance(progressed(), names(&["e.jpg"]));
    assert!(matches!(
        sidecar_plan(seen, judge(Some(local)), Some(a1), true, false),
        SidecarPlan::Push { reason: PushReason::LocalChanged, .. }
    ));
}

#[test]
fn 計画_切り離し中は自動で書かず_nasが変わったら聞き直す_b() {
    let a1 = sidecar(Some(progressed()), Some("w-a1"), "android", 10);
    let local = advance(progressed(), names(&["f.jpg"]));
    assert!(matches!(
        sidecar_plan(seen_of(&a1), judge(Some(local.clone())), Some(a1.clone()), true, true),
        SidecarPlan::Settled { seen: None, reason: SettledReason::Detached }
    ));
    let theirs = sidecar(Some(other_progress()), Some("w-x"), "pc", 30);
    assert!(matches!(
        sidecar_plan(seen_of(&a1), judge(Some(local)), Some(theirs), true, true),
        SidecarPlan::Clash { .. }
    ));
}

#[test]
fn 計画_書けない共有では端末の分を捨てない_8_3の7() {
    let a1 = sidecar(Some(progressed()), Some("w-a1"), "android", 10);
    let theirs = sidecar(Some(other_progress()), Some("w-x"), "pc", 30);
    assert!(matches!(
        sidecar_plan(seen_of(&a1), judge(Some(progressed())), Some(theirs.clone()), false, false),
        SidecarPlan::Settled { seen: None, reason: SettledReason::ReadOnly }
    ));
    // 端末が未着手なら、書けなくても取り込む。
    assert!(matches!(
        sidecar_plan(SeenRecord::default(), judge(Some(fresh())), Some(theirs), false, false),
        SidecarPlan::Pull { reason: PullReason::LocalUntouched, .. }
    ));
}

#[test]
fn 計画_新しすぎる版には書かない() {
    let local = progressed();
    let mut newer = sidecar(Some(fresh()), Some("w-n"), "future", 30);
    newer.version = SIDECAR_VERSION + 1;
    assert!(matches!(
        sidecar_plan(SeenRecord::default(), judge(Some(local)), Some(newer), true, false),
        SidecarPlan::Settled { seen: None, reason: SettledReason::NewerVersion }
    ));
}

#[test]
fn 計画_手直し_境目だけが片方にあれば確認なしに合わせる_7() {
    let a1 = sidecar(Some(progressed()), Some("w-a1"), "android", 10);
    let seen = seen_of(&a1);

    // 相手だけに手直しがある → 取り込む。
    let mut theirs = sidecar(Some(progressed()), Some("w-x"), "pc", 20);
    theirs.burst_overrides = vec![split("a.jpg", "b.jpg")];
    match sidecar_plan(seen.clone(), judge(Some(progressed())), Some(theirs), true, false) {
        SidecarPlan::Pull { aside_mine: false, reason: PullReason::TheirsHasMore, .. } => {}
        other => panic!("取り込むはず: {other:?}"),
    }

    // 端末だけに境目がある → 書く。
    let plain = sidecar(Some(progressed()), Some("w-y"), "pc", 20);
    let learned = canonical_judgement(Some(progressed()), HashMap::new(), vec![], Some(9), None);
    match sidecar_plan(seen.clone(), learned, Some(plain), true, false) {
        SidecarPlan::Push { expected, reason: PushReason::MineHasMore, .. } => {
            assert_eq!(expected.as_deref(), Some("w-y"));
        }
        other => panic!("書くはず: {other:?}"),
    }

    // 両方にあって食い違う → 確認。
    let mut joined = sidecar(Some(progressed()), Some("w-z"), "pc", 20);
    joined.burst_overrides = vec![join("a.jpg", "b.jpg")];
    let mine = canonical_judgement(Some(progressed()), HashMap::new(), vec![split("a.jpg", "b.jpg")], None, None);
    assert!(matches!(
        sidecar_plan(seen, mine, Some(joined), true, false),
        SidecarPlan::Clash { reason: ClashReason::ExtrasConflict, .. }
    ));
}

#[test]
fn 計画_相手がやり直していれば確認_9_3の13() {
    let mut a1 = sidecar(Some(progressed()), Some("w-a1"), "android", 10);
    a1.epoch = Some("e1".into());
    let local = canonical_judgement(Some(advance(progressed(), names(&["e.jpg"]))), HashMap::new(), vec![], None, Some("e1".into()));
    // PC で最初からやり直した（未着手・新しい epoch）。
    let mut restarted = sidecar(Some(fresh()), Some("w-r"), "pc", 20);
    restarted.epoch = Some("e2".into());
    restarted.based_on = Some("w-a1".into());
    match sidecar_plan(seen_of(&a1), local, Some(restarted), true, false) {
        SidecarPlan::Clash { reason: ClashReason::TheirsRestarted, .. } => {}
        other => panic!("やり直しは黙って書き戻さず確認するはず: {other:?}"),
    }
}

/// U42（ユーザー決定 2026-10-02）: ほかの端末がやり直した版は、この端末が見た版のあと
/// 何も変えていなくても（早送りの関係でも）、着手済みなら確認してから取り込む。
/// 以前は早送り（#6）が先に当たり、確認なしに端末の星と選別の途中が消えていた。
#[test]
fn 計画_相手がやり直した版は早送りの関係でも端末が着手済みなら確認_u42() {
    let mut a1 = sidecar(Some(progressed()), Some("w-a1"), "android", 10);
    a1.epoch = Some("e1".into());
    // 端末は a1 を書いたあと何も変えていない。
    let local = canonical_judgement(Some(progressed()), HashMap::new(), vec![], None, Some("e1".into()));
    assert_eq!(judgement_key(local.clone()), seen_of(&a1).key, "端末は変わっていない");

    // PC で最初からやり直した（未着手・新しい epoch・a1 の上に書いた）。
    let mut restarted = sidecar(Some(fresh()), Some("w-r"), "pc", 20);
    restarted.epoch = Some("e2".into());
    restarted.based_on = Some("w-a1".into());
    restarted.lineage = Some(vec!["w-a1".into()]);
    match sidecar_plan(seen_of(&a1), local.clone(), Some(restarted.clone()), true, false) {
        SidecarPlan::Clash { reason: ClashReason::TheirsRestarted, .. } => {}
        other => panic!("やり直しは早送りでも確認するはず: {other:?}"),
    }
    // 切り離し中でも同じ（自動では取り込まない）。
    assert!(matches!(
        sidecar_plan(seen_of(&a1), local.clone(), Some(restarted.clone()), true, true),
        SidecarPlan::Clash { reason: ClashReason::TheirsRestarted, .. }
    ));

    // やり直したあと PC が少し進めて書いた版でも同じ。
    let mut progressed_after = sidecar(Some(other_progress()), Some("w-r2"), "pc", 30);
    progressed_after.epoch = Some("e2".into());
    progressed_after.based_on = Some("w-r".into());
    progressed_after.lineage = Some(vec!["w-r".into(), "w-a1".into()]);
    assert!(matches!(
        sidecar_plan(seen_of(&a1), local.clone(), Some(progressed_after), true, false),
        SidecarPlan::Clash { reason: ClashReason::TheirsRestarted, .. }
    ));

    // 書けない共有では取り込まず、端末の分をそのまま（この端末だけの結果）。消さない側に倒す。
    assert!(matches!(
        sidecar_plan(seen_of(&a1), local, Some(restarted), false, false),
        SidecarPlan::Settled { seen: None, reason: SettledReason::ReadOnly }
    ));
}

/// U42 の副作用（安全側。意図して確かめる）: 世代の違う版の上に「書き込む」（C）を選ぶと、
/// NAS の epoch が変わるので、ほかの端末（着手済み・変えていない）からも早送りではなく確認になる。
/// epoch の違いだけでは「やり直し」と「世代をまたいだ書き込み」を見分けられないため。
#[test]
fn 計画_世代の違う版の上に書き込んだ版は相手から早送りでなく確認になる_u42() {
    // PC はやり直して（e2）少し進め、書いた。
    let mut p2 = sidecar(Some(other_progress()), Some("w-p2"), "pc", 20);
    p2.epoch = Some("e2".into());
    // Android はそれを見て「この端末の状況をサイドカーに書き込む」（e1 のまま）。
    let mut mine = sidecar(Some(progressed()), None, "android", 30);
    mine.epoch = Some("e1".into());
    let written = sidecar_stamp(mine, "w-c".into(), Some(p2.clone()));
    let pc_local = canonical_judgement(Some(other_progress()), HashMap::new(), vec![], None, Some("e2".into()));
    assert!(matches!(
        sidecar_plan(seen_of(&p2), pc_local, Some(written.clone()), true, false),
        SidecarPlan::Clash { reason: ClashReason::TheirsRestarted, .. }
    ));
    // PC が未着手（やり直した直後のまま）なら、今までどおり確認なしに取り込む。
    let pc_fresh = canonical_judgement(Some(fresh()), HashMap::new(), vec![], None, Some("e2".into()));
    let mut p1 = sidecar(Some(fresh()), Some("w-p1"), "pc", 20);
    p1.epoch = Some("e2".into());
    let mut mine = sidecar(Some(progressed()), None, "android", 30);
    mine.epoch = Some("e1".into());
    let written = sidecar_stamp(mine, "w-c2".into(), Some(p1.clone()));
    assert!(matches!(
        sidecar_plan(seen_of(&p1), pc_fresh, Some(written), true, false),
        SidecarPlan::Pull { reason: PullReason::LocalUntouched, .. }
    ));
}

/// U42 で変えないこと: 端末が未着手なら、やり直された版も確認なしに取り込む。
/// 意味が同じなら何も出さない。同じ世代の早送りは今までどおり確認なし。
#[test]
fn 計画_やり直された版でも端末が未着手か意味が同じなら確認しない_u42() {
    let mut a1 = sidecar(Some(fresh()), Some("w-a1"), "android", 10);
    a1.epoch = Some("e1".into());
    let mut restarted = sidecar(Some(other_progress()), Some("w-r"), "pc", 20);
    restarted.epoch = Some("e2".into());
    restarted.based_on = Some("w-a1".into());
    restarted.lineage = Some(vec!["w-a1".into()]);

    // 端末は未着手 → 取り込む（捨てるものが無い）。
    let untouched_local = canonical_judgement(Some(fresh()), HashMap::new(), vec![], None, Some("e1".into()));
    assert!(matches!(
        sidecar_plan(seen_of(&a1), untouched_local, Some(restarted.clone()), true, false),
        SidecarPlan::Pull { reason: PullReason::LocalUntouched, aside_mine: false, .. }
    ));

    // 端末がすでに同じ状態（同じ世代・同じ星）→ 何もしない。
    let same = sidecar_judgement(restarted.clone());
    assert!(matches!(
        sidecar_plan(seen_of(&a1), same, Some(restarted), true, false),
        SidecarPlan::Settled { reason: SettledReason::Same, .. }
    ));
}

#[test]
fn 計画_この端末がやり直した後に相手が進んでいれば取り込まず確認_3の13() {
    let mut a1 = sidecar(Some(progressed()), Some("w-a1"), "android", 10);
    a1.epoch = Some("e1".into());
    // 端末はやり直した（未着手・新しい epoch）。
    let local = canonical_judgement(Some(fresh()), HashMap::new(), vec![], None, Some("e2".into()));
    let mut b1 = sidecar(Some(advance(progressed(), names(&["e.jpg"]))), Some("w-b1"), "pc", 20);
    b1.epoch = Some("e1".into());
    b1.based_on = Some("w-a1".into());
    b1.lineage = Some(vec!["w-a1".into()]);
    assert!(matches!(
        sidecar_plan(seen_of(&a1), local, Some(b1), true, false),
        SidecarPlan::Clash { reason: ClashReason::MineRestarted, .. }
    ));
}

#[test]
fn 計画_この端末のやり直しは見た版の上なら書く_3の13() {
    let mut a1 = sidecar(Some(progressed()), Some("w-a1"), "android", 10);
    a1.epoch = Some("e1".into());
    let local = canonical_judgement(Some(fresh()), HashMap::new(), vec![], None, Some("e2".into()));
    match sidecar_plan(seen_of(&a1), local, Some(a1), true, false) {
        SidecarPlan::Push { reason: PushReason::LocalChanged, aside_theirs, .. } => {
            assert!(aside_theirs, "着手済みの版を未着手で置き換えるときは退避する");
        }
        other => panic!("やり直しを伝えるはず: {other:?}"),
    }
}

#[test]
fn 計画_書きかけで壊れたcatalogは読めないことにして判断に渡さない_3の6_3の8() {
    // 途中で止まった書き込み（直接書き換えの途中）。中途半端に読んで上書きしない。
    let whole = sidecar_to_json(sidecar(Some(progressed()), Some("w-a1"), "android", 10));
    let half = whole[..whole.len() / 2].to_string();
    assert!(sidecar_from_json(half).is_none());
    // 空のファイルも同じ。
    assert!(sidecar_from_json(String::new()).is_none());
}

#[test]
fn 計画_同じフォルダの別プロジェクトも黙って上書きしない_3の11() {
    // 先に作ったプロジェクトが書いた版。後から作ったプロジェクトはまだ見ていない。
    let first = sidecar(Some(progressed()), Some("w-first"), "android", 10);
    // 後のプロジェクトが未着手なら取り込むだけ（失うものが無い）。
    assert!(matches!(
        sidecar_plan(SeenRecord::default(), judge(Some(fresh())), Some(first.clone()), true, false),
        SidecarPlan::Pull { reason: PullReason::LocalUntouched, .. }
    ));
    // 後のプロジェクトも着手済みなら確認（どちらも黙って消さない）。
    assert!(matches!(
        sidecar_plan(SeenRecord::default(), judge(Some(other_progress())), Some(first), true, false),
        SidecarPlan::Clash { .. }
    ));
}

#[test]
fn 計画_明示の保存も同じ判断で相手の変更を上書きしない_3の14() {
    let a1 = sidecar(Some(progressed()), Some("w-a1"), "android", 10);
    let local = advance(progressed(), names(&["e.jpg"]));
    // NAS が見た版のままなら書く（書く直前に NAS が w-a1 のままであることを確かめる）。
    assert!(matches!(
        sidecar_plan(seen_of(&a1), judge(Some(local.clone())), Some(a1.clone()), true, false),
        SidecarPlan::Push { expected: Some(ref token), .. } if token == "w-a1"
    ));
    // その間に別の端末が書いていたら、書かずに確認。
    let theirs = sidecar(Some(other_progress()), Some("w-x"), "pc", 20);
    assert!(matches!(
        sidecar_plan(seen_of(&a1), judge(Some(local)), Some(theirs), true, false),
        SidecarPlan::Clash { .. }
    ));
}

#[test]
fn 計画_取り込んだあとは落ち着き_書き込んだ版は相手から早送りになる_3の15() {
    // A（サイドカーから取り込む）: 控えを sidecar_seen にすると、次は何もしない。
    let theirs = sidecar(Some(other_progress()), Some("w-x"), "pc", 20);
    let seen = seen_of(&theirs);
    assert_eq!(seen.key, judgement_key(sidecar_judgement(theirs.clone())));
    assert!(matches!(
        sidecar_plan(seen, judge(Some(other_progress())), Some(theirs.clone()), true, false),
        SidecarPlan::Settled { .. }
    ));

    // C・D・E（書き込む）: theirs の上に書いた版は、theirs を見て変えていない端末から見て早送り。
    let merged = merge_judgements(
        judge(Some(progressed())),
        judge(Some(other_progress())),
        MergeMode::Union,
        2,
        "e".into(),
    );
    let written = sidecar_stamp(sidecar(Some(merged.session), None, "android", 30), "w-merged".into(), Some(theirs.clone()));
    match sidecar_plan(seen_of(&theirs), judge(Some(other_progress())), Some(written), true, false) {
        SidecarPlan::Pull { reason: PullReason::FastForward, aside_mine: true, .. } => {}
        other => panic!("相手は確認なしに取り込むはず: {other:?}"),
    }
}

#[test]
fn 計画_書いている間に増えた判断は次に変更ありとして残る_3の17() {
    // push を組んだ瞬間の写しから比較キーを控える（今の generation の代わり）。
    let snapshot = progressed();
    let written = sidecar_stamp(sidecar(Some(snapshot.clone()), None, "android", 10), "w-a2".into(), None);
    let seen = SeenRecord { token: "w-a2".into(), key: judgement_key(judge(Some(snapshot))), epoch: None };
    // 書いている間に 1 組進んだ。
    let later = advance(progressed(), names(&["e.jpg"]));
    assert!(matches!(
        sidecar_plan(seen, judge(Some(later)), Some(written), true, false),
        SidecarPlan::Push { reason: PushReason::LocalChanged, .. }
    ));
}

#[test]
fn 版の見分け_時計がずれても書いた版で見分ける_3の5() {
    let legacy = sidecar(None, None, "pc", 100);
    assert_eq!(sidecar_token(legacy), "legacy:100:pc");
    // 同じ時刻・同じ端末でも writeId が違えば別の版。
    let a = sidecar(None, Some("w-a"), "pc", 100);
    let b = sidecar(None, Some("w-b"), "pc", 100);
    assert_ne!(sidecar_token(a), sidecar_token(b));
    // writeId が同じなら時刻が違っても同じ版。
    let c = sidecar(None, Some("w-a"), "pc", 999);
    assert_eq!(sidecar_token(c), "w-a");
}

// ---------------------------------------------------------------------------
// 進み具合（ダイアログの表示用）
// ---------------------------------------------------------------------------

fn progress(round: u32, finished: bool, decided: u32, starred: u32) -> SidecarProgress {
    SidecarProgress { started: true, round, finished, decided, starred, ..SidecarProgress::default() }
}

#[test]
fn 進み具合_roundが先なら先_食い違えば言えない_同じなら同じ() {
    assert_eq!(progress_cmp(progress(2, false, 0, 5), progress(1, false, 40, 3)), ProgressOrder::Ahead);
    assert_eq!(progress_cmp(progress(1, false, 5, 3), progress(1, false, 10, 4)), ProgressOrder::Behind);
    assert_eq!(progress_cmp(progress(1, false, 10, 2), progress(1, false, 5, 8)), ProgressOrder::Unclear);
    assert_eq!(progress_cmp(progress(1, true, 3, 3), progress(1, true, 3, 3)), ProgressOrder::Even);
    let mut learned = progress(1, true, 3, 3);
    learned.learned = true;
    assert_eq!(progress_cmp(learned, progress(1, true, 3, 3)), ProgressOrder::Ahead);
}

// ---------------------------------------------------------------------------
// 混ぜ方（D・E。§4.6.2・§4.6.3）
// ---------------------------------------------------------------------------

/// 終わった選別（全部判定済み）。
/// 本物のセッションと同じく、`ratings` には全部の写真（NAMES）を載せ、★0 も持つ（D3 の修正で、
/// セッションに載っていない写真は「その端末に無い＝未判定」と扱うようになったため）。
fn session_ratings(ratings: &[(&str, i32)]) -> HashMap<String, i32> {
    let mut all: HashMap<String, i32> = NAMES.iter().map(|name| (name.to_string(), 0)).collect();
    all.extend(stars(ratings));
    all
}

fn done(ratings: &[(&str, i32)]) -> Session {
    let mut session = fresh();
    session.queue.clear();
    session.current.clear();
    session.finished = true;
    session.ratings = session_ratings(ratings);
    session
}

/// 途中の選別。`remaining` がまだ見ていない写真。
fn midway(ratings: &[(&str, i32)], remaining: &[&str]) -> Session {
    let mut session = fresh();
    session.current = names(remaining);
    session.queue.clear();
    session.finished = false;
    session.ratings = session_ratings(ratings);
    session.history = vec![crate::Decision {
        group: names(&["a.jpg", "b.jpg"]),
        chosen: names(&["a.jpg"]),
        topped: None,
        before: HashMap::new(),
    }];
    session
}

fn mine_theirs() -> (Judgement, Judgement) {
    let mine = judge(Some(done(&[("a.jpg", 2), ("b.jpg", 1), ("d.jpg", 1)])));
    let theirs = judge(Some(midway(&[("a.jpg", 1), ("sub/c.jpg", 1)], &["d.jpg"])));
    (mine, theirs)
}

#[test]
fn 混ぜ方_和集合は大きい方の星() {
    let (mine, theirs) = mine_theirs();
    let merged = merge_stars(mine, theirs, MergeMode::Union);
    assert_eq!(merged, stars(&[("a.jpg", 2), ("b.jpg", 1), ("sub/c.jpg", 1), ("d.jpg", 1)]));
}

#[test]
fn 混ぜ方_積集合は判定済みどうしなら小さい方_片方が未判定なら判定済みの側() {
    let (mine, theirs) = mine_theirs();
    let merged = merge_stars(mine, theirs, MergeMode::Intersection);
    // a: min(2,1)=1。b: 相手は判定済みで 0 → 0。c: 端末は判定済みで 0 → 0。
    // d: 相手はまだ見ていない → 端末の 1。
    assert_eq!(merged, stars(&[("a.jpg", 1), ("d.jpg", 1)]));
}

#[test]
fn 混ぜ方_積集合で両方とも未判定なら小さい方() {
    let mine = judge(Some(midway(&[("e.jpg", 2)], &["e.jpg"])));
    let theirs = judge(Some(midway(&[("e.jpg", 1)], &["e.jpg"])));
    assert_eq!(merge_stars(mine, theirs, MergeMode::Intersection), stars(&[("e.jpg", 1)]));
}

#[test]
fn 混ぜ方_セッションが無く星も0の写真は未判定() {
    // 相手は Session 無し（星だけ）。d は相手で 0 → 未判定 → 端末の星を採る。
    let mine = judge(Some(done(&[("a.jpg", 1), ("d.jpg", 3)])));
    let theirs = canonical_judgement(None, stars(&[("a.jpg", 2)]), vec![], None, None);
    assert_eq!(
        merge_stars(mine, theirs, MergeMode::Intersection),
        stars(&[("a.jpg", 1), ("d.jpg", 3)])
    );
}

#[test]
fn 混ぜ方_セッションは混ぜた星の完了状態になり続きを始められる() {
    let (mine, theirs) = mine_theirs();
    let result = merge_judgements(mine, theirs, MergeMode::Intersection, 3, "e-new".into());
    assert!(result.session.finished);
    assert!(result.session.history.is_empty());
    assert!(result.session.queue.is_empty() && result.session.current.is_empty());
    assert_eq!(result.session.round, 1);
    assert_eq!(result.session.group_size, 3);
    assert_eq!(result.starred, 2);
    // U45: undecided は「混ぜたあとも残る、どちらも見ていない写真」。d は相手がまだ見ていないが、
    // この端末は完了（判定済み）なので残りは 0 → 完了状態のまま（以前の期待は 1）。
    assert_eq!(result.undecided, 0);
    assert_eq!(result.ratings, stars(&[("a.jpg", 1), ("d.jpg", 1)]));
    assert_eq!(result.session.ratings, result.ratings);
    // ★1 の 2 枚でもう一度選別できる。
    let again = round_for(result.session.clone(), refs(&NAMES), 1, false, thr(), vec![]).expect("始められるはず");
    assert_eq!(again.round, 2);
    assert_eq!(again.current, names(&["a.jpg", "d.jpg"]));
    // 次のラウンドの対象（survivors）は target を超え ★5 未満のもの。
    let mut survivors = result.session.survivors.clone();
    survivors.sort();
    assert_eq!(survivors, names(&["a.jpg", "d.jpg"]));
}

#[test]
fn 混ぜ方_完了状態はroundとtargetの大きい方() {
    let mut late = done(&[("a.jpg", 3), ("b.jpg", 2)]);
    late.round = 3;
    late.target_star = 2;
    let mine = judge(Some(late));
    let theirs = judge(Some(done(&[("a.jpg", 1)])));
    let result = merge_judgements(mine, theirs, MergeMode::Union, 2, "e-new".into());
    assert_eq!(result.session.round, 3);
    assert_eq!(result.session.target_star, 2);
    assert_eq!(result.session.survivors, names(&["a.jpg"]));
}

#[test]
fn 混ぜ方_星5は次のラウンドの対象にしない() {
    let session = session_from_ratings(stars(&[("a.jpg", 5), ("b.jpg", 1)]), 1, 0, 2, HashMap::new());
    assert_eq!(session.survivors, names(&["b.jpg"]));
}

#[test]
fn 混ぜ方_手直しは和集合で食い違えば端末_境目は端末か有る方_世代() {
    let mine = canonical_judgement(
        Some(done(&[("a.jpg", 1)])),
        HashMap::new(),
        vec![split("a.jpg", "b.jpg"), join("sub/c.jpg", "d.jpg")],
        None,
        Some("e1".into()),
    );
    let theirs = canonical_judgement(
        Some(done(&[("a.jpg", 1)])),
        HashMap::new(),
        vec![join("a.jpg", "b.jpg"), split("e.jpg", "f.jpg")],
        Some(7),
        Some("e1".into()),
    );
    let result = merge_judgements(mine.clone(), theirs.clone(), MergeMode::Union, 2, "e-new".into());
    assert_eq!(
        result.overrides,
        vec![split("a.jpg", "b.jpg"), split("e.jpg", "f.jpg"), join("sub/c.jpg", "d.jpg")]
    );
    assert_eq!(result.burst_distance, Some(7));
    assert_eq!(result.epoch.as_deref(), Some("e1"), "同じ世代ならそのまま");

    let mut mine_learned = mine;
    mine_learned.burst_distance = Some(9);
    mine_learned.epoch = Some("e3".into());
    let result = merge_judgements(mine_learned, theirs, MergeMode::Union, 2, "e-new".into());
    assert_eq!(result.burst_distance, Some(9), "両方にあれば端末の値");
    assert_eq!(result.epoch.as_deref(), Some("e-new"), "世代が違えば新しい世代");
    assert_eq!(
        merge_overrides(vec![split("a.jpg", "b.jpg")], vec![join("a.jpg", "b.jpg")]),
        vec![split("a.jpg", "b.jpg")]
    );
}

#[test]
fn 混ぜ方_見込みの枚数() {
    let (mine, theirs) = mine_theirs();
    let preview = merge_preview(mine.clone(), theirs.clone());
    assert_eq!(preview.mine_starred, 3);
    assert_eq!(preview.theirs_starred, 2);
    assert_eq!(preview.intersection_starred, 2);
    assert_eq!(preview.union_starred, 4);
    // U45: 端末は完了しているので、どちらも見ていない写真は無い（以前の期待は 1）。
    assert_eq!(preview.undecided, 0);
    assert!(preview.mid_round);
    assert!(preview.mergeable);
    let union = merge_judgements(mine, theirs, MergeMode::Union, 2, "e".into());
    assert_eq!(union.starred, preview.union_starred);
}

/// D3（レビュー 2026-10-02）: PC は RAW・HEIC も選別の対象にし、Android は jpg/png/webp だけ。
/// 片方の記録（セッションの ratings・photos）に無い写真は、その端末では「未判定」なので、
/// 積集合でも記録のある側の★を採る（ユーザー決定「片方で未判定の写真は、判定済みの側の★」）。
#[test]
fn 混ぜ方_積集合で片方の記録に無い写真は記録のある側の星_d3() {
    // PC: JPG と RAW の両方を選別した（完了）。
    let mut pc_session = done(&[("a.jpg", 2), ("b.jpg", 1)]);
    pc_session.ratings.insert("a.cr2".into(), 2);
    pc_session.ratings.insert("b.heic".into(), 1);
    pc_session.ratings.insert("c.arw".into(), 0);
    let pc = judge(Some(pc_session));
    // Android: JPG だけ（RAW・HEIC は記録に無い）。
    let android = judge(Some(done(&[("a.jpg", 1), ("d.jpg", 1)])));

    let expected = stars(&[("a.jpg", 1), ("a.cr2", 2), ("b.heic", 1)]);
    assert_eq!(merge_stars(android.clone(), pc.clone(), MergeMode::Intersection), expected, "Android から");
    assert_eq!(merge_stars(pc.clone(), android.clone(), MergeMode::Intersection), expected, "PC から");
    // 和集合は今までどおり大きい方（記録に無い側は 0 と同じ）。
    assert_eq!(
        merge_stars(android.clone(), pc.clone(), MergeMode::Union),
        stars(&[("a.jpg", 2), ("b.jpg", 1), ("d.jpg", 1), ("a.cr2", 2), ("b.heic", 1)])
    );
    // 見込みと結果は同じ数え方（ダイアログの数のまま混ざる）。
    let preview = merge_preview(android.clone(), pc.clone());
    assert_eq!(preview.intersection_starred, 3);
    let result = merge_judgements(android, pc, MergeMode::Intersection, 2, "e".into());
    assert_eq!(result.ratings, expected);
    assert_eq!(result.starred, preview.intersection_starred);
}

/// D3: セッションが無い側（★だけの古い記録）でも、photos に無い写真は未判定（今までどおり）。
/// photos に★0 で載っている写真は、セッションが無ければ未判定のまま（設計書どおり）。
#[test]
fn 混ぜ方_d3でも記録の比べ方は意味が同じかの判断に影響しない() {
    // 記録の顔ぶれ（★0 の写真）だけ違う 2 つは、今までどおり「意味が同じ」（警告なし）。
    let mut pc_session = done(&[("a.jpg", 1)]);
    pc_session.ratings.insert("a.cr2".into(), 0);
    let pc = judge(Some(pc_session));
    let android = judge(Some(done(&[("a.jpg", 1)])));
    assert!(judgement_equivalent(pc.clone(), android.clone()));
    assert_eq!(judgement_key(pc), judgement_key(android));
}

// ---------------------------------------------------------------------------
// U45: 混ぜても、どちらの端末もまだ見ていない写真をスキップしない
// ---------------------------------------------------------------------------

fn many(count: usize) -> Vec<String> {
    (0..count).map(|index| format!("p{index:04}.jpg")).collect()
}

fn refs_named(list: &[String]) -> Vec<PhotoRef> {
    list.iter()
        .enumerate()
        .map(|(index, name)| PhotoRef {
            relative_path: name.clone(),
            captured_at: Some(index as i64 * 100_000),
            d_hash: Some("0000000000000000".into()),
            d_hash_version: 2,
        })
        .collect()
}

/// `groups` 組を決める。各組の `pick` 番目を選ぶ。
fn judge_groups(mut session: Session, groups: usize, pick: usize) -> Session {
    for _ in 0..groups {
        let chosen = session.current[pick].clone();
        session = advance(session, vec![chosen]);
    }
    session
}

fn remaining_of(session: &Session) -> Vec<String> {
    session.current.iter().chain(&session.queue).cloned().collect()
}

/// 報告（L:\20260927_NANIWA_SPLASH_Day2\Saya）の形。844 枚・4 枚ずつ。
/// A（この端末）は ROUND 1 で 100 組（400 枚）を決め、残り 444 枚。
/// B（NAS）は同じ ROUND 1 で 80 組（320 枚）を決め、残り 524 枚（A の残り 444 枚を含む）。
fn reported_case() -> (Vec<String>, Session, Session) {
    let all = many(844);
    let start = start_round(refs_named(&all), 4, 0, false, thr(), vec![]);
    let a = judge_groups(start.clone(), 100, 0);
    let b = judge_groups(start, 80, 1);
    assert_eq!(remaining_of(&a).len(), 444);
    assert_eq!(remaining_of(&b).len(), 524);
    (all, a, b)
}

#[test]
fn 混ぜ方_u45_和集合でもどちらも見ていない444枚はスキップせず続きから選別できる() {
    let (all, a, b) = reported_case();
    let unseen: Vec<String> = all[400..].to_vec();
    let preview = merge_preview(judge(Some(a.clone())), judge(Some(b.clone())));
    assert!(preview.mergeable, "同じ ROUND・同じ対象★なら混ぜられる");
    assert!(preview.mid_round);
    assert_eq!(preview.undecided, 444, "混ぜたあとも残る、どちらも見ていない写真の数");

    let result = merge_judgements(judge(Some(a)), judge(Some(b)), MergeMode::Union, 4, "e".into());
    let session = result.session.clone();
    assert!(!session.finished, "未判定の写真が残るので完了にしない");
    assert_eq!(remaining_of(&session), unseen, "両方が見ていない写真が、元の並び順のまま全部残る");
    assert_eq!(session.current.len(), 4, "先頭の 1 組を出している");
    assert!(session.history.is_empty(), "1 つ戻すはできない");
    assert_eq!((session.round, session.target_star, session.group_size), (1, 0, 4));
    assert_eq!(result.undecided, 444);
    // ★: A が選んだ 100 枚と B が選んだ 80 枚（和集合）。
    let expected: HashMap<String, i32> = (0..100)
        .map(|k| (all[4 * k].clone(), 1))
        .chain((0..80).map(|k| (all[4 * k + 1].clone(), 1)))
        .collect();
    assert_eq!(result.ratings, expected);
    assert_eq!(result.starred, 180);
    // survivors（次のラウンドの対象）も和集合。
    let mut survivors: Vec<String> = expected.keys().cloned().collect();
    survivors.sort();
    assert_eq!(session.survivors, survivors);
    // 判定済みの写真は queue に出ない。
    assert!(all[..400].iter().all(|id| !remaining_of(&session).contains(id)));

    // 続きから選別できる（先頭から出て、選べば★が上がり、最後まで進める）。
    let next = advance(session, vec![all[400].clone()]);
    assert_eq!(next.ratings.get(&all[400]).copied(), Some(1));
    assert_eq!(next.current, all[404..408].to_vec());
    let mut rest = next;
    let mut groups = 1;
    while !rest.finished {
        rest = advance(rest, vec![]);
        groups += 1;
    }
    assert_eq!(groups, 111, "444 枚を 4 枚ずつ全部見る");
    assert!(rest.survivors.contains(&all[400]));
}

#[test]
fn 混ぜ方_u45_積集合でも残りは続きから_片方だけが判定した写真はその側の判断() {
    let (all, a, b) = reported_case();
    let result = merge_judgements(judge(Some(a)), judge(Some(b)), MergeMode::Intersection, 4, "e".into());
    assert!(!result.session.finished);
    assert_eq!(remaining_of(&result.session), all[400..].to_vec());
    // 先頭 80 組は両方が判定し、選んだ写真が違う → どちらも★0。
    // 81〜100 組目は A だけが判定 → A の判断（各組の先頭が★1）。
    let expected: HashMap<String, i32> = (80..100).map(|k| (all[4 * k].clone(), 1)).collect();
    assert_eq!(result.ratings, expected);
    let mut survivors: Vec<String> = expected.keys().cloned().collect();
    survivors.sort();
    assert_eq!(result.session.survivors, survivors);
}

#[test]
fn 混ぜ方_u45_片方だけが判定した写真はその側の判断で_残りは両方の未判定だけ() {
    // A: [a,b] で a を選んだ（残り sub/c・d・e・f）。
    // B: [a,b] はどれも選ばず、[sub/c,d] で d を選んだ（残り e・f）。
    let a = advance(fresh(), names(&["a.jpg"]));
    let b = advance(advance(fresh(), names(&[])), names(&["d.jpg"]));
    // 積集合: a は両方が判定（B は選ばず）→ 0。d は B だけが判定 → 1。sub/c も B だけ → 0。
    let result = merge_judgements(judge(Some(a.clone())), judge(Some(b.clone())), MergeMode::Intersection, 2, "e".into());
    assert!(!result.session.finished);
    assert_eq!(remaining_of(&result.session), names(&["e.jpg", "f.jpg"]));
    assert_eq!(result.ratings, stars(&[("d.jpg", 1)]));
    assert_eq!(result.session.survivors, names(&["d.jpg"]));
    // 和集合: a（A で選んだ）と d（B で選んだ）。
    let result = merge_judgements(judge(Some(a)), judge(Some(b)), MergeMode::Union, 2, "e".into());
    assert_eq!(remaining_of(&result.session), names(&["e.jpg", "f.jpg"]));
    assert_eq!(result.ratings, stars(&[("a.jpg", 1), ("d.jpg", 1)]));
    assert_eq!(result.session.survivors, names(&["a.jpg", "d.jpg"]));
    // 判定していない写真の★0 も記録に残す（次に混ぜるとき「記録に無い＝未判定」にならない）。
    assert_eq!(result.session.ratings.get("e.jpg").copied(), Some(0));
}

/// 連写のまとまり: 代表 a に b、代表 e に f。代表だけが並ぶ（a・sub/c・d・e）。
fn burst_start() -> Session {
    let mut session = fresh();
    session.members = HashMap::from([
        ("a.jpg".to_string(), names(&["a.jpg", "b.jpg"])),
        ("e.jpg".to_string(), names(&["e.jpg", "f.jpg"])),
    ]);
    session.current = names(&["a.jpg", "sub/c.jpg"]);
    session.queue = names(&["d.jpg", "e.jpg"]);
    session
}

#[test]
fn 混ぜ方_u45_連写の仲間も判定済みとして扱い_残りのまとまりは仲間ごと続きから() {
    let a = advance(burst_start(), names(&["a.jpg"]));
    let b = advance(burst_start(), names(&[]));
    let preview = merge_preview(judge(Some(a.clone())), judge(Some(b.clone())));
    assert_eq!(preview.undecided, 3, "d と、まとまり e（e・f）");

    let result = merge_judgements(judge(Some(a)), judge(Some(b)), MergeMode::Union, 2, "e".into());
    assert!(!result.session.finished);
    // b は a の仲間として判定済み → 出さない。e のまとまりは代表だけが出る。
    assert_eq!(remaining_of(&result.session), names(&["d.jpg", "e.jpg"]));
    assert_eq!(result.session.members.get("e.jpg"), Some(&names(&["e.jpg", "f.jpg"])));
    assert_eq!(result.ratings, stars(&[("a.jpg", 1), ("b.jpg", 1)]));
    // 続きで e を選ぶと、仲間の f にも★が配られる。
    let next = advance(result.session, names(&["e.jpg"]));
    assert_eq!(next.ratings.get("f.jpg").copied(), Some(1));
    assert!(next.finished);
}

#[test]
fn 混ぜ方_u45_roundか対象の星が違えば混ぜられず_呼ばれても端末の分を変えない() {
    // この端末: ROUND 2（★1 から）の途中。NAS: ROUND 1 の途中。
    let mut late = fresh();
    late.round = 2;
    late.target_star = 1;
    late.ratings = session_ratings(&[("a.jpg", 1), ("b.jpg", 1), ("d.jpg", 2)]);
    late.current = names(&["a.jpg", "b.jpg"]);
    late.queue = vec![];
    late.history = vec![crate::Decision {
        group: names(&["d.jpg", "e.jpg"]),
        chosen: names(&["d.jpg"]),
        topped: None,
        before: HashMap::new(),
    }];
    late.survivors = names(&["d.jpg"]);
    let mine = judge(Some(late));
    let theirs = judge(Some(other_progress()));

    let preview = merge_preview(mine.clone(), theirs.clone());
    assert!(!preview.mergeable, "途中の ROUND があり、ROUND が違う");
    // 呼ばれても（画面が古いなど）、この端末の分をそのまま返す（スキップしない）。
    let result = merge_judgements(mine.clone(), theirs.clone(), MergeMode::Union, 2, "e".into());
    assert!(!result.session.finished);
    assert_eq!(remaining_of(&result.session), names(&["a.jpg", "b.jpg"]));
    assert_eq!((result.session.round, result.session.target_star), (2, 1));
    assert_eq!(result.ratings, stars(&[("a.jpg", 1), ("b.jpg", 1), ("d.jpg", 2)]));
    assert_eq!(result.session.survivors, names(&["d.jpg"]));

    // 片方が完了していても、もう片方が途中で ROUND が違えば混ぜない。
    let mut finished_late = done(&[("a.jpg", 2)]);
    finished_late.round = 2;
    finished_late.target_star = 1;
    assert!(!merge_preview(judge(Some(finished_late)), theirs).mergeable);
}

#[test]
fn 混ぜ方_u45_両方完了なら今までどおり完了状態で見込みの未判定は0() {
    let mut late = done(&[("a.jpg", 3), ("b.jpg", 2)]);
    late.round = 3;
    late.target_star = 2;
    let mine = judge(Some(late));
    let theirs = judge(Some(done(&[("a.jpg", 1)])));
    let preview = merge_preview(mine.clone(), theirs.clone());
    assert!(preview.mergeable, "未判定が無いので、ROUND が違っても混ぜられる");
    assert!(!preview.mid_round);
    assert_eq!(preview.undecided, 0);
    let result = merge_judgements(mine, theirs, MergeMode::Union, 2, "e".into());
    assert!(result.session.finished);
    assert_eq!(result.undecided, 0);
}

// ---------------------------------------------------------------------------
// 写真の鍵（§4.8）
// ---------------------------------------------------------------------------

/// Android の形（共有の根からの相対・フォルダ名付き）の Session。連写のまとまり・
/// ★5・戻すの控えまで入れて、中の鍵が全部変わることを確かめる。
fn android_session(prefix: &str) -> Session {
    let names_with_prefix: Vec<String> = NAMES.iter().map(|name| format!("{prefix}/{name}")).collect();
    let photos: Vec<PhotoRef> = names_with_prefix
        .iter()
        .enumerate()
        .map(|(index, name)| PhotoRef {
            relative_path: name.clone(),
            // 先頭の 3 枚は連写（近い時刻・同じ値）。
            captured_at: Some(if index < 3 { index as i64 * 1000 } else { index as i64 * 100_000 }),
            d_hash: Some("0000000000000000".into()),
            d_hash_version: 2,
        })
        .collect();
    let session = start_round(photos, 2, 0, true, thr(), vec![]);
    let first = session.current[0].clone();
    let session = keep_top(session, first);
    let next = session.current[0].clone();
    advance(session, vec![next])
}

fn android_sidecar(prefix: &str) -> Sidecar {
    let mut sidecar = sidecar(Some(android_session(prefix)), None, "android", 1);
    sidecar.key_base = None;
    sidecar.burst_overrides = vec![split(&format!("{prefix}/a.jpg"), &format!("{prefix}/b.jpg"))];
    sidecar.photos.insert(format!("{prefix}/e.jpg"), SidecarPhoto { rating: 0 });
    sidecar
}

#[test]
fn 鍵_android形式からフォルダ形式へ変えて戻すと元どおり() {
    let prefix = "photo/2021_06_13";
    let original = android_sidecar(prefix);
    assert!(!original.sessions.tournament.as_ref().unwrap().members.is_empty());
    assert!(original.sessions.tournament.as_ref().unwrap().history[0].topped.is_some());

    let folder = sidecar_keys_to_folder(original.clone(), prefix.into());
    assert_eq!(folder.key_base.as_deref(), Some(KEY_BASE_FOLDER));
    let json = sidecar_to_json(folder.clone());
    assert!(!json.contains("photo/2021_06_13"), "中の鍵が全部変わる: {json}");
    assert!(folder.photos.contains_key("e.jpg"));
    assert!(folder.sessions.tournament.as_ref().unwrap().ratings.contains_key("sub/c.jpg"));

    let back = sidecar_keys_from_folder(folder, prefix.into(), "/".into());
    let before = original.sessions.tournament.clone().unwrap();
    let after = back.sessions.tournament.clone().unwrap();
    assert_eq!(before.queue, after.queue);
    assert_eq!(before.current, after.current);
    assert_eq!(before.survivors, after.survivors);
    assert_eq!(before.ratings, after.ratings);
    assert_eq!(before.members, after.members);
    assert_eq!(before.history[0].group, after.history[0].group);
    assert_eq!(before.history[0].chosen, after.history[0].chosen);
    assert_eq!(before.history[0].before, after.history[0].before);
    assert_eq!(
        before.history[0].topped.as_ref().map(|t| t.path.clone()),
        after.history[0].topped.as_ref().map(|t| t.path.clone())
    );
    assert_eq!(original.burst_overrides, back.burst_overrides);
    assert!(judgement_equivalent(sidecar_judgement(original), sidecar_judgement(back)));
}

#[test]
fn 鍵_pcの区切りをスラッシュにして戻せる() {
    let mut pc = sidecar(None, None, "pc", 1);
    pc.photos.insert("sub\\c.jpg".into(), SidecarPhoto { rating: 2 });
    let folder = sidecar_keys_to_folder(pc, String::new());
    assert!(folder.photos.contains_key("sub/c.jpg"));
    let back = sidecar_keys_from_folder(folder, String::new(), "\\".into());
    assert!(back.photos.contains_key("sub\\c.jpg"));
}

#[test]
fn 鍵_古いandroid形式は推定で頭を外す() {
    let prefix = "photo/2021_06_13";
    let expected = sidecar_judgement(sidecar_keys_to_folder(android_sidecar(prefix), prefix.into()));
    // Android 自身（フォルダ = 共有の根からの相対）。
    let by_android = sidecar_normalize_keys(android_sidecar(prefix), prefix.into());
    assert_eq!(by_android.key_base.as_deref(), Some(KEY_BASE_FOLDER));
    assert!(judgement_equivalent(sidecar_judgement(by_android), expected.clone()));
    // PC（選んだフォルダの絶対パス。末尾が同じ）。
    let by_pc = sidecar_normalize_keys(android_sidecar(prefix), "\\\\NAS\\share\\photo\\2021_06_13".into());
    assert!(judgement_equivalent(sidecar_judgement(by_pc), expected));
}

#[test]
fn 鍵_推定は頭が合わなければ外さない() {
    let prefix = "photo/2021_06_13";
    let other = sidecar_normalize_keys(android_sidecar(prefix), "C:\\Users\\me\\Pictures".into());
    assert!(other.photos.contains_key("photo/2021_06_13/e.jpg"));
    // PC の古い形で、全部がサブフォルダ（名前が選んだフォルダと違う）。
    let mut pc = sidecar(None, None, "pc", 1);
    pc.key_base = None;
    pc.photos.insert("sub\\x.jpg".into(), SidecarPhoto { rating: 1 });
    pc.photos.insert("sub\\y.jpg".into(), SidecarPhoto { rating: 1 });
    let pc = sidecar_normalize_keys(pc, "D:\\pics".into());
    assert!(pc.photos.contains_key("sub/x.jpg"));
}

#[test]
fn 鍵_keybaseがfolderなら推定しない() {
    let mut sidecar = sidecar(None, None, "pc", 1);
    sidecar.photos.insert("photo/2021_06_13/a.jpg".into(), SidecarPhoto { rating: 1 });
    let same = sidecar_normalize_keys(sidecar, "photo/2021_06_13".into());
    assert!(same.photos.contains_key("photo/2021_06_13/a.jpg"));
}

#[test]
fn 鍵_一致率を数える_3の12() {
    let mut sidecar = sidecar(None, None, "pc", 1);
    for name in ["a.jpg", "b.jpg", "sub\\c.jpg"] {
        sidecar.photos.insert(name.into(), SidecarPhoto { rating: 0 });
    }
    let coverage = sidecar_key_coverage(sidecar, names(&["a.jpg", "sub/c.jpg", "z.jpg"]));
    assert_eq!(coverage, KeyCoverage { matched: 2, total: 3 });
}

// ---------------------------------------------------------------------------
// catalog.json v2 と古い版との互換（§4.9）
// ---------------------------------------------------------------------------

/// U33 より前の core の `Sidecar` と同じ形（古い版のアプリの読み方）。
#[derive(Debug, serde::Deserialize)]
#[allow(dead_code)]
struct OldSidecar {
    #[serde(default)]
    version: i32,
    #[serde(rename = "updatedAt", default)]
    updated_at: i64,
    #[serde(rename = "updatedBy", default)]
    updated_by: String,
    #[serde(rename = "updatedByName", default)]
    updated_by_name: String,
    #[serde(default)]
    photos: HashMap<String, SidecarPhoto>,
    #[serde(rename = "burstOverrides", default)]
    burst_overrides: Vec<PairOverride>,
    #[serde(default)]
    sessions: SidecarSessions,
    #[serde(rename = "burstDistance", default)]
    burst_distance: Option<u32>,
}

fn stamped() -> Sidecar {
    let mut local = sidecar(Some(progressed()), None, "android", 50);
    local.photos.insert("a.jpg".into(), SidecarPhoto { rating: 1 });
    let mut base = sidecar(Some(fresh()), Some("w-b"), "pc", 40);
    base.lineage = Some(vec!["w-a".into()]);
    sidecar_stamp(local, "w-new".into(), Some(base))
}

#[test]
fn 互換_versionの無い古いcatalogがそのまま読める() {
    let json = r#"{
        "updatedAt": 5,
        "updatedBy": "old",
        "photos": { "a.jpg": { "rating": 2 } },
        "burstOverrides": [ { "l": "a.jpg", "r": "b.jpg", "d": "join" } ],
        "sessions": {}
    }"#;
    let sidecar = sidecar_from_json(json.into()).expect("読めるはず");
    assert_eq!(sidecar.version, 1);
    assert!(sidecar.write_id.is_none() && sidecar.based_on.is_none() && sidecar.lineage.is_none());
    assert!(sidecar.epoch.is_none() && sidecar.key_base.is_none() && sidecar.progress.is_none());
    assert_eq!(sidecar_token(sidecar.clone()), "legacy:5:old");
    let judgement = sidecar_judgement(sidecar.clone());
    assert_eq!(judgement.stars, vec![PhotoStar { path: "a.jpg".into(), rating: 2 }]);
    assert!(matches!(
        sidecar_plan(SeenRecord::default(), Judgement::default(), Some(sidecar), true, false),
        SidecarPlan::Pull { .. }
    ));
}

#[test]
fn 互換_古い版のアプリは新しい版を読んでも壊れない() {
    let json = sidecar_to_json(stamped());
    assert!(json.contains("\"writeId\"") && json.contains("\"progress\"") && json.contains("\"lineage\""));
    let old: OldSidecar = serde_json::from_str(&json).expect("古い形でも読めるはず");
    assert_eq!(old.version, SIDECAR_VERSION);
    assert_eq!(old.photos.get("a.jpg").map(|p| p.rating), Some(1));
    assert!(old.sessions.tournament.is_some());
}

#[test]
fn 互換_新しい項目を持たない版は書いても新しい項目が出ない() {
    let mut legacy = sidecar(Some(fresh()), None, "pc", 1);
    legacy.key_base = None;
    let json = sidecar_to_json(legacy);
    for field in ["writeId", "basedOn", "lineage", "epoch", "keyBase", "progress"] {
        assert!(!json.contains(field), "{field} は出さない: {json}");
    }
}

#[test]
fn 互換_新しい項目の型が違っても全体は読める() {
    let json = r#"{
        "version": 2,
        "updatedAt": 5,
        "updatedBy": "x",
        "writeId": 12,
        "basedOn": null,
        "lineage": "broken",
        "epoch": ["e"],
        "keyBase": {},
        "progress": "broken",
        "photos": { "a.jpg": { "rating": 2 } },
        "sessions": {}
    }"#;
    let sidecar = sidecar_from_json(json.into()).expect("新しい項目が壊れていても読めるはず");
    assert!(sidecar.write_id.is_none() && sidecar.lineage.is_none() && sidecar.progress.is_none());
    assert!(sidecar.epoch.is_none() && sidecar.key_base.is_none() && sidecar.based_on.is_none());
    assert_eq!(sidecar.photos.get("a.jpg").map(|p| p.rating), Some(2));
}

#[test]
fn 刻印_書く前にversion2と版の見分けと要約を入れる() {
    let sidecar = stamped();
    assert_eq!(sidecar.version, SIDECAR_VERSION);
    assert_eq!(sidecar.write_id.as_deref(), Some("w-new"));
    assert_eq!(sidecar.based_on.as_deref(), Some("w-b"));
    assert_eq!(sidecar.lineage.clone().unwrap(), names(&["w-b", "w-a"]));
    assert_eq!(sidecar.key_base.as_deref(), Some(KEY_BASE_FOLDER));
    let progress = sidecar.progress.clone().unwrap();
    assert!(progress.started);
    assert_eq!(progress.round, 1);
    assert_eq!(progress.decided, 2);
    assert_eq!(progress.starred, 3);
    assert_eq!(progress.remaining, 2);
    assert_eq!(progress.total, Some(6));
    // 書いて読み戻しても同じ。
    let back = sidecar_from_json(sidecar_to_json(sidecar.clone())).unwrap();
    assert_eq!(back.write_id, sidecar.write_id);
    assert_eq!(back.lineage, sidecar.lineage);
    assert_eq!(back.progress, sidecar.progress);
    assert_eq!(sidecar_token(back), "w-new");
}

#[test]
fn 刻印_古い形の版の上に書くとlegacyの見分けを控える_系統は32まで() {
    let legacy = sidecar(None, None, "pc", 7);
    let stamped = sidecar_stamp(sidecar(None, None, "me", 8), "w-1".into(), Some(legacy));
    assert_eq!(stamped.based_on.as_deref(), Some("legacy:7:pc"));
    assert_eq!(stamped.lineage.clone().unwrap(), names(&["legacy:7:pc"]));

    let first = sidecar_stamp(sidecar(None, None, "me", 8), "w-0".into(), None);
    assert!(first.based_on.is_none());
    let mut long = sidecar(None, Some("w-top"), "pc", 9);
    long.lineage = Some((0..40).map(|index| format!("w-{index}")).collect());
    let capped = sidecar_stamp(sidecar(None, None, "me", 10), "w-next".into(), Some(long));
    let lineage = capped.lineage.unwrap();
    assert_eq!(lineage.len(), 32);
    assert_eq!(lineage[0], "w-top");
}

// ---------------------------------------------------------------------------
// プロジェクトの設定の同期（U48）: settings.pairRawJpeg
// ---------------------------------------------------------------------------

fn pair(value: bool, at: i64) -> Option<SettingsRecord> {
    Some(SettingsRecord { pair_raw_jpeg: Some(SettingValueBool { value, at }), other: HashMap::new() })
}

#[test]
fn 設定_どちらかが無ければある方を採る_u48() {
    assert_eq!(settings_resolve(None, None), SettingsPlan::Keep);
    assert_eq!(settings_resolve(None, pair(false, 5)), SettingsPlan::AdoptRemote { value: false, at: 5 });
    assert_eq!(settings_resolve(pair(false, 5), None), SettingsPlan::PushLocal);
    // settings はあっても pairRawJpeg が無いのは、無いのと同じ。
    let empty = Some(SettingsRecord::default());
    assert_eq!(settings_resolve(pair(true, 0), empty.clone()), SettingsPlan::PushLocal);
    assert_eq!(settings_resolve(empty, pair(true, 0)), SettingsPlan::AdoptRemote { value: true, at: 0 });
}

#[test]
fn 設定_値が同じなら時刻が違っても何もしない_u48() {
    assert_eq!(settings_resolve(pair(true, 0), pair(true, 99)), SettingsPlan::Keep);
    assert_eq!(settings_resolve(pair(false, 99), pair(false, 1)), SettingsPlan::Keep);
}

#[test]
fn 設定_値が違えば新しく切り替えた方_同じ時刻ならnas_u48() {
    assert_eq!(settings_resolve(pair(false, 20), pair(true, 10)), SettingsPlan::PushLocal);
    assert_eq!(settings_resolve(pair(false, 10), pair(true, 20)), SettingsPlan::AdoptRemote { value: true, at: 20 });
    // 作ったまま（時刻 0）の端末は、切り替えた記録に合わせる。
    assert_eq!(settings_resolve(pair(true, 0), pair(false, 1)), SettingsPlan::AdoptRemote { value: false, at: 1 });
    assert_eq!(settings_resolve(pair(true, 7), pair(false, 7)), SettingsPlan::AdoptRemote { value: false, at: 7 });
}

/// 古い版（settings なし）の catalog.json は読めて、書き戻しても他が変わらず、settings も足さない。
#[test]
fn 設定_settingsなしの古い版を読んで書き戻しても不変_u48() {
    let text = r#"{"version":2,"updatedAt":5,"updatedBy":"pc","updatedByName":"PC","photos":{"a.jpg":{"rating":1}},
        "burstOverrides":[{"left":"a.jpg","right":"b.jpg","decision":"split"}],"sessions":{},"burstDistance":9,
        "writeId":"w-1","basedOn":"w-0","lineage":["w-0"],"epoch":"e-1","keyBase":"folder"}"#;
    let sidecar = sidecar_from_json(text.into()).expect("読める");
    assert!(sidecar.settings.is_none());
    let back: serde_json::Value = serde_json::from_str(&sidecar_to_json(sidecar)).unwrap();
    let original: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(back, original);
    assert!(back.get("settings").is_none());
}

#[test]
fn 設定_読み書きの形と未知の設定を保つ_u48() {
    let text = r#"{"version":2,"updatedAt":5,"updatedBy":"pc","updatedByName":"PC","photos":{},"burstOverrides":[],
        "sessions":{},"settings":{"pairRawJpeg":{"value":false,"at":1790955613101},"futureThing":{"value":3,"at":4,"x":[1]}}}"#;
    let sidecar = sidecar_from_json(text.into()).expect("読める");
    let settings = sidecar.settings.clone().expect("settings");
    assert_eq!(settings.pair_raw_jpeg, Some(SettingValueBool { value: false, at: 1790955613101 }));
    assert!(settings.other.contains_key("futureThing"));
    let back: serde_json::Value = serde_json::from_str(&sidecar_to_json(sidecar)).unwrap();
    let original: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(back["settings"], original["settings"]);
}

#[test]
fn 設定_型の違う値は読み捨てて全体は読める_u48() {
    let base = r#""version":2,"updatedAt":5,"updatedBy":"pc","updatedByName":"PC","photos":{"a.jpg":{"rating":2}},"burstOverrides":[],"sessions":{}"#;
    let broken = sidecar_from_json(format!(r#"{{{base},"settings":5}}"#)).expect("settings が数でも読める");
    assert!(broken.settings.is_none());
    assert_eq!(broken.photos["a.jpg"].rating, 2);
    let odd = sidecar_from_json(format!(r#"{{{base},"settings":{{"pairRawJpeg":"yes","other":{{"value":1}}}}}}"#))
        .expect("pairRawJpeg の型が違っても読める");
    let settings = odd.settings.expect("settings");
    assert!(settings.pair_raw_jpeg.is_none());
    assert!(settings.other.contains_key("other"));
    // at が無ければ 0（一度も切り替えていない）。
    let no_at = sidecar_from_json(format!(r#"{{{base},"settings":{{"pairRawJpeg":{{"value":true}}}}}}"#)).unwrap();
    assert_eq!(no_at.settings.unwrap().pair_raw_jpeg, Some(SettingValueBool { value: true, at: 0 }));
}

#[test]
fn 設定_書く前の刻印で置き換える版の未知の設定と端末に無い設定を引き継ぐ_u48() {
    let mut base = sidecar(None, Some("w-1"), "android", 5);
    let mut other = HashMap::new();
    other.insert("futureThing".to_string(), r#"{"value":3}"#.to_string());
    base.settings = Some(SettingsRecord { pair_raw_jpeg: Some(SettingValueBool { value: false, at: 9 }), other });

    // 端末に設定が無い（古い書き手）→ NAS の分をそのまま引き継ぐ。
    let kept = sidecar_stamp(sidecar(None, None, "me", 6), "w-2".into(), Some(base.clone()));
    let settings = kept.settings.expect("引き継ぐ");
    assert_eq!(settings.pair_raw_jpeg, Some(SettingValueBool { value: false, at: 9 }));
    assert!(settings.other.contains_key("futureThing"));

    // 端末に設定がある → 端末の値を書き、未知の設定は残す。
    let mut mine = sidecar(None, None, "me", 6);
    mine.settings = pair(true, 12);
    let stamped = sidecar_stamp(mine, "w-3".into(), Some(base));
    let settings = stamped.settings.clone().expect("settings");
    assert_eq!(settings.pair_raw_jpeg, Some(SettingValueBool { value: true, at: 12 }));
    assert_eq!(settings.other.get("futureThing").map(String::as_str), Some(r#"{"value":3}"#));
    let json: serde_json::Value = serde_json::from_str(&sidecar_to_json(stamped)).unwrap();
    assert_eq!(json["settings"]["futureThing"]["value"], 3);
    assert_eq!(json["settings"]["pairRawJpeg"], serde_json::json!({ "value": true, "at": 12 }));
}

#[test]
fn 設定_比較キーと意味が同じかの判断に入れない_u48() {
    let plain = sidecar(Some(progressed()), Some("w-1"), "pc", 5);
    let mut with = plain.clone();
    with.settings = pair(false, 123);
    let mut other_value = plain.clone();
    other_value.settings = pair(true, 456);
    for candidate in [&with, &other_value] {
        assert!(judgement_equivalent(sidecar_judgement(plain.clone()), sidecar_judgement(candidate.clone())));
        assert_eq!(judgement_key(sidecar_judgement(plain.clone())), judgement_key(sidecar_judgement(candidate.clone())));
        assert_eq!(sidecar_seen(plain.clone()), sidecar_seen(candidate.clone()));
    }
}

/// 設定だけを書き直した版（writeId は新しい・選別状況は見た版のまま）は、確認も「早送りの取り込み」も起こさない。
#[test]
fn 計画_設定だけ書き直された版の上には端末の変更をそのまま書く_u48() {
    let a1 = sidecar(Some(progressed()), Some("w-a1"), "android", 10);
    let seen = seen_of(&a1);
    // PC が設定だけ変えて書いた版。
    let mut p2 = a1.clone();
    p2.updated_by = "pc".into();
    p2.settings = pair(false, 99);
    let p2 = sidecar_stamp(p2, "w-p2".into(), Some(a1.clone()));

    // 端末が変わっていなければ「意味が同じ」で何もしない（控えだけ進める）。
    assert!(matches!(
        sidecar_plan(seen.clone(), judge(Some(progressed())), Some(p2.clone()), true, false),
        SidecarPlan::Settled { reason: SettledReason::Same, .. }
    ));
    // 端末が進んでいれば、確認せずに書く（NAS の選別状況は見た版から変わっていない）。
    let changed = advance(progressed(), names(&["e.jpg"]));
    match sidecar_plan(seen.clone(), judge(Some(changed.clone())), Some(p2.clone()), true, false) {
        SidecarPlan::Push { expected, aside_theirs: false, reason: PushReason::LocalChanged } => {
            assert_eq!(expected.as_deref(), Some("w-p2"));
        }
        other => panic!("書くはず: {other:?}"),
    }
    // 切り離し中・書けない共有なら書かない（#3 と同じ）。
    assert!(matches!(
        sidecar_plan(seen.clone(), judge(Some(changed.clone())), Some(p2.clone()), true, true),
        SidecarPlan::Settled { reason: SettledReason::Detached, .. }
    ));
    assert!(matches!(
        sidecar_plan(seen, judge(Some(changed)), Some(p2), false, false),
        SidecarPlan::Settled { reason: SettledReason::ReadOnly, .. }
    ));
}
