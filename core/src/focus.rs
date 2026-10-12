//! ピントが合っている所の位置（フォーカスピーキング）。
//!
//! 輝度の局所的な勾配（ラプラシアン）が強い所を「ピントが合っている」と見る。
//! 結果は**長辺 512px 以下のマスク**で、値は 0〜255 の強さ（そのままアルファに使える）。
//! DB には残さない（呼ぶたびに計算する）。
//!
//! 入力は長辺 2048px までを想定する。それより大きいものは呼ぶ側で縮める
//! （超えていても落ちないが、重くなるだけ）。

/// 出力マスクの長辺の上限。
pub const FOCUS_MASK_MAX_SIDE: u32 = 512;
/// 絶対の下限（ラプラシアンの強さ、0〜1020）。これに届かない所は何があっても出さない。
/// 全体がぼけた写真・のっぺりした写真で、ほぼ何も出なくするための値。
const ABSOLUTE_FLOOR: u32 = 24;
/// 相対のしきい値。強い順に上位これだけ（‰）を残す。
const KEEP_PER_MILLE: usize = 30;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusMask {
    pub width: u32,
    pub height: u32,
    /// `width * height` 個。0 は「ピントが合っていない」、大きいほど強い。
    pub data: Vec<u8>,
}

impl FocusMask {
    fn empty() -> Self {
        FocusMask { width: 0, height: 0, data: Vec::new() }
    }
}

/// 輝度（`width * height` 個）から、ピントが合っている所のマスクを作る。
///
/// 読めない入力（0 サイズ・3 未満・長さの不一致）では**空のマスク**（0×0）を返す。
pub fn focus_mask(gray: Vec<u8>, width: u32, height: u32) -> FocusMask {
    let (w, h) = (width as usize, height as usize);
    if w < 3 || h < 3 || w.checked_mul(h) != Some(gray.len()) {
        return FocusMask::empty();
    }

    // 1 点だけのノイズを拾わないよう、先に 3x3 で均してからラプラシアンを取る。
    // 均した値は 9 倍のまま持つ（整数のまま計算するため）。
    let mut horizontal = vec![0u16; w * h];
    for y in 0..h {
        let row = &gray[y * w..(y + 1) * w];
        for x in 0..w {
            let left = row[x.saturating_sub(1)] as u16;
            let right = row[(x + 1).min(w - 1)] as u16;
            horizontal[y * w + x] = left + row[x] as u16 + right;
        }
    }
    let mut smooth = vec![0u16; w * h];
    for y in 0..h {
        let up = y.saturating_sub(1);
        let down = (y + 1).min(h - 1);
        for x in 0..w {
            smooth[y * w + x] = horizontal[up * w + x] + horizontal[y * w + x] + horizontal[down * w + x];
        }
    }
    drop(horizontal);

    // 強さ（0〜1020）。縁の 1 画素は隣が無いので 0。
    let mut strength = vec![0u16; w * h];
    let mut histogram = vec![0usize; 1021];
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let at = y * w + x;
            let lap = 4 * smooth[at] as i32
                - smooth[at - 1] as i32
                - smooth[at + 1] as i32
                - smooth[at - w] as i32
                - smooth[at + w] as i32;
            let value = (lap.unsigned_abs() / 9).min(1020) as u16;
            strength[at] = value;
            histogram[value as usize] += 1;
        }
    }

    // 相対: 強い順に上位 KEEP_PER_MILLE‰ を残す値。同じ強さが並ぶ場合は含める（>=）。
    let total = (w - 2) * (h - 2);
    let wanted = (total * KEEP_PER_MILLE / 1000).max(1);
    let mut seen = 0usize;
    let mut relative = 0u32;
    for value in (0..histogram.len()).rev() {
        seen += histogram[value];
        if seen >= wanted {
            relative = value as u32;
            break;
        }
    }
    let threshold = relative.max(ABSOLUTE_FLOOR);

    // 最大値プーリングで 512px 以下へ。しきい値を超えた所だけ強さを持つ。
    let side = w.max(h) as u32;
    let block = side.div_ceil(FOCUS_MASK_MAX_SIDE).max(1) as usize;
    let out_w = w.div_ceil(block);
    let out_h = h.div_ceil(block);
    let mut data = vec![0u8; out_w * out_h];
    for y in 0..h {
        for x in 0..w {
            let value = strength[y * w + x] as u32;
            if value < threshold {
                continue;
            }
            // 通った所は 96〜255。しきい値の 2 倍で頭打ち。
            let over = ((value - threshold) * 159 / threshold.max(1)).min(159);
            let alpha = (96 + over) as u8;
            let cell = &mut data[(y / block) * out_w + x / block];
            if alpha > *cell {
                *cell = alpha;
            }
        }
    }
    FocusMask { width: out_w as u32, height: out_h as u32, data }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 左半分: くっきりした市松（4px 升）、右半分: ぼけた（なだらかな）縞。
    fn half_sharp(width: u32, height: u32) -> Vec<u8> {
        let mut gray = vec![0u8; (width * height) as usize];
        for y in 0..height {
            for x in 0..width {
                gray[(y * width + x) as usize] = if x < width / 2 {
                    if ((x / 4) + (y / 4)) % 2 == 0 { 20 } else { 235 }
                } else {
                    // ゆるい勾配（1 画素あたり 1 しか変わらない）
                    (110.0 + 30.0 * ((x as f32) / 64.0).sin()) as u8
                };
            }
        }
        gray
    }

    #[test]
    fn 左がくっきり右がぼけた画像ではマスクが左に集中する() {
        let mask = focus_mask(half_sharp(256, 128), 256, 128);
        assert_eq!((mask.width, mask.height), (256, 128));
        let (mut left, mut right) = (0u64, 0u64);
        for y in 0..mask.height {
            for x in 0..mask.width {
                let v = mask.data[(y * mask.width + x) as usize] as u64;
                if x < mask.width / 2 { left += v } else { right += v }
            }
        }
        assert!(left > 0, "左に出ていない");
        assert_eq!(right, 0, "ぼけた側に出ている: {right}");
    }

    #[test]
    fn 一様な画像は空() {
        let mask = focus_mask(vec![128; 100 * 80], 100, 80);
        assert_eq!((mask.width, mask.height), (100, 80));
        assert!(mask.data.iter().all(|&v| v == 0));
    }

    #[test]
    fn ぼけただけの画像は絶対の下限で空() {
        // 全体がなだらかな勾配。相対だけなら上位が残ってしまう。
        let gray: Vec<u8> = (0..200 * 100).map(|i| (100 + (i % 200) / 8) as u8).collect();
        let mask = focus_mask(gray, 200, 100);
        assert!(mask.data.iter().all(|&v| v == 0));
    }

    #[test]
    fn ノイズの1点だけでは光らない() {
        let mut gray = vec![100u8; 64 * 64];
        gray[32 * 64 + 32] = 160;
        let mask = focus_mask(gray, 64, 64);
        assert!(mask.data.iter().all(|&v| v == 0));
    }

    #[test]
    fn 長辺512以下へ縮めて返す() {
        let gray = half_sharp(2048, 1024);
        let mask = focus_mask(gray, 2048, 1024);
        assert_eq!((mask.width, mask.height), (512, 256));
        assert_eq!(mask.data.len(), 512 * 256);
        assert!(mask.data.iter().any(|&v| v > 0));
    }

    #[test]
    fn 割り切れない大きさでも端まで覆う() {
        let mask = focus_mask(vec![0; 1000 * 700], 1000, 700);
        assert_eq!((mask.width, mask.height), (500, 350));
        let mask = focus_mask(vec![0; 1025 * 10], 1025, 10);
        assert_eq!((mask.width, mask.height), (342, 4));
    }

    #[test]
    fn 読めない入力でも落ちない() {
        for (len, w, h) in [(0, 0, 0), (0, 10, 0), (100, 0, 100), (4, 2, 2), (9, 3, 3), (50, 10, 10), (10, 10, 1)] {
            let mask = focus_mask(vec![200; len], w, h);
            assert!(mask.data.len() == (mask.width * mask.height) as usize);
        }
        assert_eq!(focus_mask(vec![1; 5], 3, 3), FocusMask { width: 0, height: 0, data: vec![] });
        assert_eq!(focus_mask(vec![], u32::MAX, u32::MAX).width, 0);
    }

    #[test]
    fn 全面がくっきりでも出る() {
        let width = 64u32;
        let gray: Vec<u8> = (0..width * 64)
            .map(|i| if ((i % width) / 4 + (i / width) / 4) % 2 == 0 { 20 } else { 235 })
            .collect();
        let mask = focus_mask(gray, width, 64);
        assert!(mask.data.iter().any(|&v| v > 0));
    }
}
