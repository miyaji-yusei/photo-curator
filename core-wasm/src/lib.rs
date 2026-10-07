//! core の判断を PC・Web の画面（Nuxt）から呼ぶための橋。
//!
//! **判断そのものはここに書かない。** UDL の関数全部と、旧いサイドカーの判断
//! （`sidecar_decide`。UDL には無い）を、wasm-bindgen で JS から呼べる形にして出すだけ。
//! サイドカー同期の新しい判断（U33: `sidecar_plan` など）も同じく包むだけ。
//!
//! 複雑な型（`PhotoRef` や `Session` など）は JsValue で受け渡す。
//! フィールド名は Rust のまま（スネークケース）。core の JSON（`session_to_json` の
//! 出力）と揃えるため、リネームはしない（設計 04 章 note）。

use photo_curator_core as core;
use serde::Serialize;
use wasm_bindgen::prelude::*;

fn de<T: serde::de::DeserializeOwned>(value: JsValue) -> Result<T, JsValue> {
    serde_wasm_bindgen::from_value(value).map_err(|error| JsValue::from_str(&error.to_string()))
}

/// `HashMap` を（既定の Map ではなく）ふつうの JS オブジェクトにして返し、
/// `Option::None` は `undefined` ではなく `null` にする（`session_to_json`＝serde_json を `JSON.parse` した形と揃えるため）。
fn ser<T: Serialize + ?Sized>(value: &T) -> Result<JsValue, JsValue> {
    let serializer = serde_wasm_bindgen::Serializer::json_compatible();
    value
        .serialize(&serializer)
        .map_err(|error| JsValue::from_str(&error.to_string()))
}

fn ser_option<T: Serialize>(value: Option<T>) -> Result<JsValue, JsValue> {
    match value {
        Some(v) => ser(&v),
        None => Ok(JsValue::NULL),
    }
}

// ---------------------------------------------------------------------------
// 連写
// ---------------------------------------------------------------------------

#[wasm_bindgen(js_name = hashDistance)]
pub fn hash_distance(left: String, right: String) -> u32 {
    core::hash_distance(left, right)
}

#[wasm_bindgen(js_name = isSameBurst)]
pub fn is_same_burst(pair: JsValue, threshold: JsValue) -> Result<bool, JsValue> {
    let pair: core::BurstPair = de(pair)?;
    let threshold: core::BurstThreshold = de(threshold)?;
    Ok(core::is_same_burst(pair, threshold))
}

#[wasm_bindgen(js_name = groupBursts)]
pub fn group_bursts(photos: JsValue, threshold: JsValue, overrides: JsValue) -> Result<JsValue, JsValue> {
    let photos: Vec<core::PhotoRef> = de(photos)?;
    let threshold: core::BurstThreshold = de(threshold)?;
    let overrides: Vec<core::PairOverride> = de(overrides)?;
    ser(&core::group_bursts(photos, threshold, overrides))
}

#[wasm_bindgen(js_name = dHashFromLuma)]
pub fn d_hash_from_luma(luma: Vec<u8>) -> Option<String> {
    core::d_hash_from_luma(luma)
}

#[wasm_bindgen(js_name = dHashFromGray)]
pub fn d_hash_from_gray(gray: Vec<u8>, width: u32, height: u32) -> Option<String> {
    core::d_hash_from_gray(gray, width, height)
}

#[wasm_bindgen(js_name = learnDistance)]
pub fn learn_distance(answers: JsValue, fallback: u32) -> Result<u32, JsValue> {
    let answers: Vec<core::BurstAnswer> = de(answers)?;
    Ok(core::learn_distance(answers, fallback))
}

// ---------------------------------------------------------------------------
// 選別
// ---------------------------------------------------------------------------

#[wasm_bindgen(js_name = startRound)]
pub fn start_round(
    photos: JsValue,
    group_size: u32,
    target_star: i32,
    group_bursts_on: bool,
    threshold: JsValue,
    overrides: JsValue,
) -> Result<JsValue, JsValue> {
    let photos: Vec<core::PhotoRef> = de(photos)?;
    let threshold: core::BurstThreshold = de(threshold)?;
    let overrides: Vec<core::PairOverride> = de(overrides)?;
    ser(&core::start_round(
        photos,
        group_size,
        target_star,
        group_bursts_on,
        threshold,
        overrides,
    ))
}

#[wasm_bindgen]
pub fn advance(session: JsValue, selected: Vec<String>) -> Result<JsValue, JsValue> {
    let session: core::Session = de(session)?;
    ser(&core::advance(session, selected))
}

#[wasm_bindgen]
pub fn undo(session: JsValue) -> Result<JsValue, JsValue> {
    let session: core::Session = de(session)?;
    ser(&core::undo(session))
}

#[wasm_bindgen(js_name = keepTop)]
pub fn keep_top(session: JsValue, path: String) -> Result<JsValue, JsValue> {
    let session: core::Session = de(session)?;
    ser(&core::keep_top(session, path))
}

#[wasm_bindgen(js_name = keepAndTop)]
pub fn keep_and_top(session: JsValue, selected: Vec<String>, path: String) -> Result<JsValue, JsValue> {
    let session: core::Session = de(session)?;
    ser(&core::keep_and_top(session, selected, path))
}

#[wasm_bindgen(js_name = setRepresentative)]
pub fn set_representative(session: JsValue, shown: String, wanted: String) -> Result<JsValue, JsValue> {
    let session: core::Session = de(session)?;
    ser_option(core::set_representative(session, shown, wanted))
}

#[wasm_bindgen]
pub fn resize(session: JsValue, group_size: u32) -> Result<JsValue, JsValue> {
    let session: core::Session = de(session)?;
    ser(&core::resize(session, group_size))
}

#[wasm_bindgen]
pub fn regroup(
    session: JsValue,
    photos: JsValue,
    group_bursts_on: bool,
    threshold: JsValue,
    overrides: JsValue,
) -> Result<JsValue, JsValue> {
    let session: core::Session = de(session)?;
    let photos: Vec<core::PhotoRef> = de(photos)?;
    let threshold: core::BurstThreshold = de(threshold)?;
    let overrides: Vec<core::PairOverride> = de(overrides)?;
    ser(&core::regroup(session, photos, group_bursts_on, threshold, overrides))
}

#[wasm_bindgen(js_name = nextRound)]
pub fn next_round(
    previous: JsValue,
    photos: JsValue,
    group_bursts_on: bool,
    threshold: JsValue,
    overrides: JsValue,
) -> Result<JsValue, JsValue> {
    let previous: core::Session = de(previous)?;
    let photos: Vec<core::PhotoRef> = de(photos)?;
    let threshold: core::BurstThreshold = de(threshold)?;
    let overrides: Vec<core::PairOverride> = de(overrides)?;
    ser_option(core::next_round(previous, photos, group_bursts_on, threshold, overrides))
}

#[wasm_bindgen(js_name = roundFor)]
pub fn round_for(
    previous: JsValue,
    photos: JsValue,
    star: i32,
    group_bursts_on: bool,
    threshold: JsValue,
    overrides: JsValue,
) -> Result<JsValue, JsValue> {
    let previous: core::Session = de(previous)?;
    let photos: Vec<core::PhotoRef> = de(photos)?;
    let threshold: core::BurstThreshold = de(threshold)?;
    let overrides: Vec<core::PairOverride> = de(overrides)?;
    ser_option(core::round_for(previous, photos, star, group_bursts_on, threshold, overrides))
}

// ---------------------------------------------------------------------------
// 保存
// ---------------------------------------------------------------------------

#[wasm_bindgen(js_name = sessionToJson)]
pub fn session_to_json(session: JsValue) -> Result<String, JsValue> {
    let session: core::Session = de(session)?;
    Ok(core::session_to_json(session))
}

#[wasm_bindgen(js_name = sessionFromJson)]
pub fn session_from_json(json: String) -> JsValue {
    match core::session_from_json(json) {
        Some(session) => ser(&session).unwrap_or(JsValue::NULL),
        None => JsValue::NULL,
    }
}

// ---------------------------------------------------------------------------
// サイドカー（UDL には無い。core にだけある 3 つ）
// ---------------------------------------------------------------------------

#[wasm_bindgen(js_name = sidecarToJson)]
pub fn sidecar_to_json(sidecar: JsValue) -> Result<String, JsValue> {
    let sidecar: core::Sidecar = de(sidecar)?;
    Ok(core::sidecar_to_json(sidecar))
}

#[wasm_bindgen(js_name = sidecarFromJson)]
pub fn sidecar_from_json(json: String) -> JsValue {
    match core::sidecar_from_json(json) {
        Some(sidecar) => ser(&sidecar).unwrap_or(JsValue::NULL),
        None => JsValue::NULL,
    }
}

/// `seenAt` はミリ秒。JS の数値（f64）でやり取りし、ここで i64 に直す。
#[wasm_bindgen(js_name = sidecarDecide)]
pub fn sidecar_decide(
    seen_at: f64,
    seen_by: String,
    local_changed: bool,
    remote: JsValue,
) -> Result<JsValue, JsValue> {
    let remote: Option<core::Sidecar> = if remote.is_null() || remote.is_undefined() {
        None
    } else {
        Some(de(remote)?)
    };
    ser(&core::sidecar_decide(seen_at as i64, seen_by, local_changed, remote))
}

// ---------------------------------------------------------------------------
// サイドカー同期（U33）。判断は core の sidecar_sync。ここは包むだけ。
// 型の形は core の serde のまま（Judgement などはスネークケース、Sidecar は catalog.json と
// 同じ camelCase、enum は "Intersection" のような文字列、SidecarPlan は { Settled: {...} } の形）。
// ---------------------------------------------------------------------------

/// null / undefined なら None。
fn de_option<T: serde::de::DeserializeOwned>(value: JsValue) -> Result<Option<T>, JsValue> {
    if value.is_null() || value.is_undefined() {
        Ok(None)
    } else {
        Ok(Some(de(value)?))
    }
}

#[wasm_bindgen(js_name = normalizeKey)]
pub fn normalize_key(key: String) -> String {
    core::normalize_key(key)
}

#[wasm_bindgen(js_name = canonicalJudgement)]
pub fn canonical_judgement(
    session: JsValue,
    photos: JsValue,
    overrides: JsValue,
    burst_distance: Option<u32>,
    epoch: Option<String>,
) -> Result<JsValue, JsValue> {
    let session: Option<core::Session> = de_option(session)?;
    let photos: std::collections::HashMap<String, i32> = de_option(photos)?.unwrap_or_default();
    let overrides: Vec<core::PairOverride> = de_option(overrides)?.unwrap_or_default();
    ser(&core::canonical_judgement(session, photos, overrides, burst_distance, epoch))
}

#[wasm_bindgen(js_name = sidecarJudgement)]
pub fn sidecar_judgement(sidecar: JsValue) -> Result<JsValue, JsValue> {
    ser(&core::sidecar_judgement(de(sidecar)?))
}

#[wasm_bindgen(js_name = judgementEquivalent)]
pub fn judgement_equivalent(a: JsValue, b: JsValue) -> Result<bool, JsValue> {
    Ok(core::judgement_equivalent(de(a)?, de(b)?))
}

#[wasm_bindgen(js_name = judgementKey)]
pub fn judgement_key(judgement: JsValue) -> Result<String, JsValue> {
    Ok(core::judgement_key(de(judgement)?))
}

#[wasm_bindgen(js_name = isUntouched)]
pub fn is_untouched(judgement: JsValue) -> Result<bool, JsValue> {
    Ok(core::is_untouched(de(judgement)?))
}

#[wasm_bindgen(js_name = judgementProgress)]
pub fn judgement_progress(judgement: JsValue) -> Result<JsValue, JsValue> {
    ser(&core::judgement_progress(de(judgement)?))
}

#[wasm_bindgen(js_name = progressCmp)]
pub fn progress_cmp(a: JsValue, b: JsValue) -> Result<JsValue, JsValue> {
    ser(&core::progress_cmp(de(a)?, de(b)?))
}

#[wasm_bindgen(js_name = sidecarToken)]
pub fn sidecar_token(sidecar: JsValue) -> Result<String, JsValue> {
    Ok(core::sidecar_token(de(sidecar)?))
}

#[wasm_bindgen(js_name = sidecarSeen)]
pub fn sidecar_seen(sidecar: JsValue) -> Result<JsValue, JsValue> {
    ser(&core::sidecar_seen(de(sidecar)?))
}

/// `seen` は `{ token, key, epoch }`。一度も見ていなければ `{ token: '', key: '', epoch: null }`。
/// `remote` は読めた catalog.json（`sidecarFromJson` の結果。鍵は `sidecarNormalizeKeys` 済み）か null。
#[wasm_bindgen(js_name = sidecarPlan)]
pub fn sidecar_plan(
    seen: JsValue,
    local: JsValue,
    remote: JsValue,
    writable: bool,
    detached: bool,
) -> Result<JsValue, JsValue> {
    let seen: core::SeenRecord = de_option(seen)?.unwrap_or_default();
    let local: core::Judgement = de(local)?;
    let remote: Option<core::Sidecar> = de_option(remote)?;
    ser(&core::sidecar_plan(seen, local, remote, writable, detached))
}

#[wasm_bindgen(js_name = sidecarStamp)]
pub fn sidecar_stamp(sidecar: JsValue, write_id: String, base: JsValue) -> Result<JsValue, JsValue> {
    let sidecar: core::Sidecar = de(sidecar)?;
    let base: Option<core::Sidecar> = de_option(base)?;
    ser(&core::sidecar_stamp(sidecar, write_id, base))
}

/// U48: プロジェクトの設定（pairRawJpeg）をどうするか。`local`・`remote` は catalog.json の
/// `settings` の形（`{ pairRawJpeg: { value, at } }`）か null。答えは `"Keep"`・`"PushLocal"`・
/// `{ AdoptRemote: { value, at } }`。
#[wasm_bindgen(js_name = settingsResolve)]
pub fn settings_resolve(local: JsValue, remote: JsValue) -> Result<JsValue, JsValue> {
    let local: Option<core::SettingsRecord> = de_option(local)?;
    let remote: Option<core::SettingsRecord> = de_option(remote)?;
    ser(&core::settings_resolve(local, remote))
}

/// `mode` は "Intersection"（積集合）か "Union"（和集合）。
#[wasm_bindgen(js_name = mergeStars)]
pub fn merge_stars(mine: JsValue, theirs: JsValue, mode: JsValue) -> Result<JsValue, JsValue> {
    ser(&core::merge_stars(de(mine)?, de(theirs)?, de(mode)?))
}

#[wasm_bindgen(js_name = mergeOverrides)]
pub fn merge_overrides(mine: JsValue, theirs: JsValue) -> Result<JsValue, JsValue> {
    ser(&core::merge_overrides(de(mine)?, de(theirs)?))
}

#[wasm_bindgen(js_name = sessionFromRatings)]
pub fn session_from_ratings(
    ratings: JsValue,
    round: u32,
    target_star: i32,
    group_size: u32,
    members: JsValue,
) -> Result<JsValue, JsValue> {
    let members: std::collections::HashMap<String, Vec<String>> = de_option(members)?.unwrap_or_default();
    ser(&core::session_from_ratings(de(ratings)?, round, target_star, group_size, members))
}

#[wasm_bindgen(js_name = mergeJudgements)]
pub fn merge_judgements(
    mine: JsValue,
    theirs: JsValue,
    mode: JsValue,
    group_size: u32,
    fresh_epoch: String,
) -> Result<JsValue, JsValue> {
    ser(&core::merge_judgements(de(mine)?, de(theirs)?, de(mode)?, group_size, fresh_epoch))
}

#[wasm_bindgen(js_name = mergePreview)]
pub fn merge_preview(mine: JsValue, theirs: JsValue) -> Result<JsValue, JsValue> {
    ser(&core::merge_preview(de(mine)?, de(theirs)?))
}

#[wasm_bindgen(js_name = sidecarKeysToFolder)]
pub fn sidecar_keys_to_folder(sidecar: JsValue, prefix: String) -> Result<JsValue, JsValue> {
    ser(&core::sidecar_keys_to_folder(de(sidecar)?, prefix))
}

#[wasm_bindgen(js_name = sidecarKeysFromFolder)]
pub fn sidecar_keys_from_folder(sidecar: JsValue, prefix: String, separator: String) -> Result<JsValue, JsValue> {
    ser(&core::sidecar_keys_from_folder(de(sidecar)?, prefix, separator))
}

#[wasm_bindgen(js_name = sidecarNormalizeKeys)]
pub fn sidecar_normalize_keys(sidecar: JsValue, folder_hint: String) -> Result<JsValue, JsValue> {
    ser(&core::sidecar_normalize_keys(de(sidecar)?, folder_hint))
}

#[wasm_bindgen(js_name = sidecarKeyCoverage)]
pub fn sidecar_key_coverage(sidecar: JsValue, photo_keys: Vec<String>) -> Result<JsValue, JsValue> {
    ser(&core::sidecar_key_coverage(de(sidecar)?, photo_keys))
}

// ---------------------------------------------------------------------------
// 写真のファイル名（R10）
// ---------------------------------------------------------------------------

#[wasm_bindgen(js_name = isRawName)]
pub fn is_raw_name(name: String) -> bool {
    core::is_raw_name(&name)
}

#[wasm_bindgen(js_name = rawExtensions)]
pub fn raw_extensions() -> Vec<String> {
    core::raw_extensions()
}

/// 道筋ごとの「組の RAW なので除く」（boolean の配列）。
#[wasm_bindgen(js_name = pairedRawMask)]
pub fn paired_raw_mask(paths: Vec<String>) -> Result<JsValue, JsValue> {
    ser(&core::paired_raw_mask(paths))
}

#[wasm_bindgen(js_name = skipPairedRaw)]
pub fn skip_paired_raw(paths: Vec<String>, enabled: bool) -> Vec<String> {
    core::skip_paired_raw(paths, enabled)
}
