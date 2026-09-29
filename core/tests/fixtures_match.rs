//! core/tests/fixtures/ の入力を cargo test 側から検算する。
//!
//! **同じフィクスチャを vitest（core-wasm 経由）側も読む。** 両方がここに
//! 書いてある `expected` と一致すれば、Rust と wasm で答えが揃っている
//! ことになる（設計 07 章 段1 1-4）。
//!
//! フィクスチャの中身を変えるときは、このファイルと
//! `tests/core-wasm-fixtures.test.ts` の両方を見ること。

use photo_curator_core::{
    advance, d_hash_from_gray, group_bursts, learn_distance, start_round, undo, BurstAnswer,
    BurstGroup, BurstThreshold, PairOverride, PhotoRef, Session,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct GroupBurstsFixture {
    photos: Vec<PhotoRef>,
    threshold: BurstThreshold,
    overrides: Vec<PairOverride>,
    expected: Vec<BurstGroup>,
}

#[test]
fn group_bursts_はフィクスチャの期待値と一致する() {
    let fixture: GroupBurstsFixture =
        serde_json::from_str(include_str!("fixtures/group_bursts.json")).expect("fixture を読める");
    let actual = group_bursts(fixture.photos, fixture.threshold, fixture.overrides);
    assert_eq!(
        serde_json::to_value(&actual).unwrap(),
        serde_json::to_value(&fixture.expected).unwrap()
    );
}

#[derive(Deserialize)]
struct RoundFixture {
    photos: Vec<PhotoRef>,
    group_size: u32,
    target_star: i32,
    group_bursts_on: bool,
    threshold: BurstThreshold,
    overrides: Vec<PairOverride>,
    selected: Vec<String>,
    expected_after_start: Session,
    expected_after_advance: Session,
    expected_after_undo: Session,
}

#[test]
fn start_round_advance_undo_はフィクスチャの期待値と一致する() {
    let fixture: RoundFixture =
        serde_json::from_str(include_str!("fixtures/round_advance_undo.json"))
            .expect("fixture を読める");

    let after_start = start_round(
        fixture.photos.clone(),
        fixture.group_size,
        fixture.target_star,
        fixture.group_bursts_on,
        fixture.threshold.clone(),
        fixture.overrides.clone(),
    );
    assert_eq!(
        serde_json::to_value(&after_start).unwrap(),
        serde_json::to_value(&fixture.expected_after_start).unwrap(),
        "start_round"
    );

    let after_advance = advance(after_start, fixture.selected.clone());
    assert_eq!(
        serde_json::to_value(&after_advance).unwrap(),
        serde_json::to_value(&fixture.expected_after_advance).unwrap(),
        "advance"
    );

    let after_undo = undo(after_advance);
    assert_eq!(
        serde_json::to_value(&after_undo).unwrap(),
        serde_json::to_value(&fixture.expected_after_undo).unwrap(),
        "undo"
    );
}

#[derive(Deserialize)]
struct LearnDistanceFixture {
    answers: Vec<BurstAnswer>,
    fallback: u32,
    expected: u32,
}

#[test]
fn learn_distance_はフィクスチャの期待値と一致する() {
    let fixture: LearnDistanceFixture =
        serde_json::from_str(include_str!("fixtures/learn_distance.json"))
            .expect("fixture を読める");
    let actual = learn_distance(fixture.answers, fixture.fallback);
    assert_eq!(actual, fixture.expected);
}

#[derive(Deserialize)]
struct DHashFromGrayFixture {
    gray: Vec<u8>,
    width: u32,
    height: u32,
    expected: String,
}

#[test]
fn d_hash_from_gray_はフィクスチャの期待値と一致する() {
    let fixture: DHashFromGrayFixture =
        serde_json::from_str(include_str!("fixtures/d_hash_from_gray.json"))
            .expect("fixture を読める");
    let actual = d_hash_from_gray(fixture.gray, fixture.width, fixture.height);
    assert_eq!(actual, Some(fixture.expected));
}
