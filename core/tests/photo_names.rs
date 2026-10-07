//! R1 の表（`fixtures/photo-names.json`）のうち、core が持つ「組の RAW を除く」規則（R10）を検算する。
//!
//! 候補にするかどうか（拡張子の一覧・隠し名・動画）は各環境の仕事なので、ここでは
//! 各ケースの `pair_off`（設定オフのとき候補になる名前＝候補の集合）を入力にして、
//! `skip_paired_raw` が `pair_on` を返すかを見る。`known_differences` は候補の差（B4・B8）
//! だけで、組の規則の差ではないので見ない。

use photo_curator_core::{is_raw_name, paired_raw_mask, skip_paired_raw};

fn names(case: &serde_json::Value, key: &str) -> Vec<String> {
    case[key]
        .as_array()
        .unwrap_or_else(|| panic!("{key}"))
        .iter()
        .map(|value| value.as_str().expect("name").to_string())
        .collect()
}

fn sorted(mut values: Vec<String>) -> Vec<String> {
    values.sort();
    values
}

#[test]
fn photo_names_fixture_pairs_raw_like_every_implementation() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/photo-names.json")).expect("fixture parses");
    let cases = fixture["cases"].as_array().expect("cases");
    assert!(cases.len() >= 10);
    for case in cases {
        let id = case["id"].as_str().expect("id");
        let candidates = names(case, "pair_off");
        let pair_on = names(case, "pair_on");
        assert_eq!(
            sorted(skip_paired_raw(candidates.clone(), true)),
            sorted(pair_on.clone()),
            "case {id} / pair_on"
        );
        assert_eq!(
            skip_paired_raw(candidates.clone(), false),
            candidates,
            "case {id} / pair_off"
        );
        // 除かれたものは全部 RAW。
        let mask = paired_raw_mask(candidates.clone());
        for (name, skip) in candidates.iter().zip(mask) {
            assert_eq!(skip, !pair_on.contains(name), "case {id} / {name}");
            if skip {
                assert!(is_raw_name(name), "case {id} / {name} is raw");
            }
        }
    }
}
