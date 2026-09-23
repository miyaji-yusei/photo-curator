//! core の判断を PC・Web の画面（Nuxt）から呼ぶための橋。
//!
//! **判断そのものはここに書かない。** UDL の関数全部と、サイドカーの 3 つ
//! （`sidecar_to_json` / `sidecar_from_json` / `sidecar_decide`）を、
//! wasm-bindgen で JS から呼べる形にして出すだけ。
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
