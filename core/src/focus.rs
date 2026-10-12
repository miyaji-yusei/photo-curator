//! ピントが合っている「領域」（AF 枠のような四角）を見つける。
//!
//! 輪郭に点々と色をのせるのではなく、**くっきりした所をまとまりとして四角で返す**。
//! 結果は最大 3 個の四角（0〜1 に正規化した x, y, w, h と強さ）。**0 個もある**
//! （「くっきりした所が見つかりません」＝全体が甘い写真・のっぺりした写真）。
//! DB には残さない（呼ぶたびに計算する）。
//!
//! # くっきりさの尺度
//!
//! 写真を長辺 28 マスの格子に分け、マスごとに「**再ぼかし比**」を出す。
//! 勾配エネルギー（`|∇I|²` の和）を、元の画像 A と、A をさらにぼかした B で比べ、
//! `sharpness = 1 - E(B) / E(A)`（0〜1）とする。
//!
//! - 鋭い境目はぼかすと勾配が大きく減る（エネルギーは境目の幅に反比例）→ 1 に近い。
//! - すでにぼけた太い境目はぼかしても変わらない → 0 に近い。
//! - 比なので**コントラストには左右されない**。強い明暗差のぼけた境目より、
//!   弱い明暗差でも鋭い境目のほうが高くなる。
//!
//! ただし比だけでは「ほぼ何も無いマス」「ノイズだけのマス」が高く出てしまうので、
//! B の勾配エネルギーが [`MIN_BLURRED_ENERGY`] に届かないマスは**判定しない**
//! （ノイズはぼかすと消えるので、B に境目が残っているマスだけ見る）。
//!
//! # 絶対のしきい値
//!
//! 判定できたマスのうち最良でも [`SHARPNESS_FLOOR`] に届かなければ領域は 0 個。
//! 届いたときだけ、最良の [`BEST_RATIO`] 倍以上のマスを 8 近傍でつなげて外接の四角にする。
//!
//! 入力は長辺 2048px までを想定する（超えても落ちないが重くなるだけ）。
//! **入力は原本を縮めたもの**なので、軽いぼけは縮小で消えて見える。そのためしきい値は
//! 厳しめ（ぼけた所は出にくい側）に置いている。実写真で見ながら下の定数だけ動かせばよい。

use serde::Serialize;

/// 格子の長辺のマス数。1 マスは長辺の約 3.6 %（1024px なら 37px）。
/// 顔の中の目鼻が 1〜数マスに収まる細かさ。これより細かいと「点々」に戻る。
pub const GRID_LONG_SIDE_CELLS: usize = 28;
/// 返す四角の最大数。
pub const MAX_REGIONS: usize = 3;
/// B（再ぼかし後）の勾配エネルギーの 1 画素あたり平均がこれ未満のマスは判定しない。
/// 輝度の段差 40 の鋭い境目が 1 本通るマスで 3〜5 程度。ノイズ（振れ幅 ±12）は約 0.5。
pub const MIN_BLURRED_ENERGY: f32 = 1.5;
/// 絶対のしきい値。再ぼかし比のくっきりさ（0〜1）がこれに届かなければ「くっきりした所なし」。
/// 縮小後の鋭い境目（にじみ幅 σ≈0.5〜0.8px）は 0.55〜0.67、σ=1.2px 以上のぼけは 0.35 以下になる。
/// 縮小で軽いぼけが消える分を見込み、少し厳しめの 0.45（σ≈0.95px 相当）。
pub const SHARPNESS_FLOOR: f32 = 0.45;
/// 最良のマスのこの倍以上を「最良に近い」として領域にする。
pub const BEST_RATIO: f32 = 0.65;
/// 孤立した 1 マスは、これ以上のときだけ領域にする（ノイズ 1 点を拾わないため）。
pub const SINGLE_CELL_MIN: f32 = 0.7;
/// 四角の一辺の最小（長辺に対する割合）。小さい領域も AF 枠として見える大きさに広げる。
pub const MIN_REGION_SIDE: f32 = 0.06;

/// 再ぼかしの回数。1 回は 5 点の二項フィルタ `[1 4 6 4 1] / 16`（σ=1）。2 回で σ≈1.41。
const BLUR_PASSES: usize = 2;
/// 1 マスで判定に使う画素数の下限（端の欠けたマスを除く）。
const MIN_CELL_PIXELS: u32 = 16;

/// くっきりした四角 1 個。座標と大きさは 0〜1（写真全体に対する割合）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FocusRegion {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// くっきりさ（0〜1）。大きいほど強い。
    pub strength: f32,
}

/// くっきりした四角たち。強い順で最大 [`MAX_REGIONS`] 個。空なら「くっきりした所なし」。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FocusRegions {
    pub regions: Vec<FocusRegion>,
}

/// 輝度（`width * height` 個）から、くっきりした領域の四角を返す。
///
/// 読めない入力（3 未満・長さの不一致）では**空**を返す（落ちない）。
pub fn focus_regions(gray: Vec<u8>, width: u32, height: u32) -> FocusRegions {
    let none = FocusRegions { regions: Vec::new() };
    let (w, h) = (width as usize, height as usize);
    if w < 3 || h < 3 || w.checked_mul(h) != Some(gray.len()) {
        return none;
    }

    let fine: Vec<f32> = gray.iter().map(|&v| v as f32).collect();
    let mut coarse = fine.clone();
    for _ in 0..BLUR_PASSES {
        coarse = blur5(&coarse, w, h);
    }

    let cell = w.max(h).div_ceil(GRID_LONG_SIDE_CELLS).max(1);
    let (cols, rows) = (w.div_ceil(cell), h.div_ceil(cell));
    let scores = cell_scores(&fine, &coarse, w, h, cell, cols, rows);

    let best = scores.iter().flatten().fold(0.0f32, |m, &s| m.max(s));
    if best < SHARPNESS_FLOOR {
        return none;
    }
    let cutoff = SHARPNESS_FLOOR.max(best * BEST_RATIO);

    let mut rects: Vec<Rect> = components(&scores, cols, rows, cutoff)
        .into_iter()
        .filter(|c| c.count > 1 || c.best >= SINGLE_CELL_MIN)
        .map(|c| c.to_rect(cell, w, h))
        .collect();
    merge_overlapping(&mut rects);
    rects.sort_by(|a, b| b.strength.total_cmp(&a.strength));
    rects.truncate(MAX_REGIONS);

    FocusRegions {
        regions: rects
            .into_iter()
            .map(|r| r.widened(w, h).into_region())
            .collect(),
    }
}

/// `[1 4 6 4 1] / 16` を横・縦にかける（端は端の値で延ばす）。
fn blur5(src: &[f32], w: usize, h: usize) -> Vec<f32> {
    const K: [f32; 5] = [1.0 / 16.0, 4.0 / 16.0, 6.0 / 16.0, 4.0 / 16.0, 1.0 / 16.0];
    let mut horizontal = vec![0.0f32; w * h];
    for y in 0..h {
        let row = &src[y * w..(y + 1) * w];
        for x in 0..w {
            let mut sum = 0.0;
            for (i, k) in K.iter().enumerate() {
                let at = (x + i).saturating_sub(2).min(w - 1);
                sum += k * row[at];
            }
            horizontal[y * w + x] = sum;
        }
    }
    let mut out = vec![0.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut sum = 0.0;
            for (i, k) in K.iter().enumerate() {
                let at = (y + i).saturating_sub(2).min(h - 1);
                sum += k * horizontal[at * w + x];
            }
            out[y * w + x] = sum;
        }
    }
    out
}

/// マスごとのくっきりさ。判定しないマス（境目が無い・画素が足りない）は `None`。
fn cell_scores(
    fine: &[f32],
    coarse: &[f32],
    w: usize,
    h: usize,
    cell: usize,
    cols: usize,
    rows: usize,
) -> Vec<Option<f32>> {
    let mut fine_energy = vec![0.0f64; cols * rows];
    let mut coarse_energy = vec![0.0f64; cols * rows];
    let mut pixels = vec![0u32; cols * rows];
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let at = y * w + x;
            let slot = (y / cell) * cols + x / cell;
            let gf = (fine[at + 1] - fine[at - 1]).powi(2) + (fine[at + w] - fine[at - w]).powi(2);
            let gc = (coarse[at + 1] - coarse[at - 1]).powi(2) + (coarse[at + w] - coarse[at - w]).powi(2);
            // 中心差分は 2 画素ぶんの差なので、勾配は 1/2、エネルギーは 1/4。
            fine_energy[slot] += gf as f64 * 0.25;
            coarse_energy[slot] += gc as f64 * 0.25;
            pixels[slot] += 1;
        }
    }
    (0..cols * rows)
        .map(|slot| {
            let n = pixels[slot];
            if n < MIN_CELL_PIXELS || fine_energy[slot] <= 0.0 {
                return None;
            }
            if (coarse_energy[slot] / n as f64) < MIN_BLURRED_ENERGY as f64 {
                return None;
            }
            Some((1.0 - coarse_energy[slot] / fine_energy[slot]).clamp(0.0, 1.0) as f32)
        })
        .collect()
}

/// マス単位の外接（両端を含む）。
struct Component {
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
    count: usize,
    best: f32,
}

/// 実際の座標（0〜1）の四角。
#[derive(Clone, Copy)]
struct Rect {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    strength: f32,
}

impl Component {
    fn to_rect(&self, cell: usize, w: usize, h: usize) -> Rect {
        Rect {
            x0: (self.x0 * cell) as f32 / w as f32,
            y0: (self.y0 * cell) as f32 / h as f32,
            x1: (((self.x1 + 1) * cell).min(w)) as f32 / w as f32,
            y1: (((self.y1 + 1) * cell).min(h)) as f32 / h as f32,
            strength: self.best.clamp(0.0, 1.0),
        }
    }
}

impl Rect {
    fn intersects(&self, other: &Rect) -> bool {
        self.x0 <= other.x1 && other.x0 <= self.x1 && self.y0 <= other.y1 && other.y0 <= self.y1
    }

    /// 小さすぎる四角を、中心を保ったまま最小の大きさに広げる（0〜1 に収める）。
    fn widened(mut self, w: usize, h: usize) -> Rect {
        let long = w.max(h) as f32;
        let min_w = (MIN_REGION_SIDE * long / w as f32).min(1.0);
        let min_h = (MIN_REGION_SIDE * long / h as f32).min(1.0);
        for (lo, hi, min) in [(&mut self.x0, &mut self.x1, min_w), (&mut self.y0, &mut self.y1, min_h)] {
            if *hi - *lo >= min {
                continue;
            }
            let mid = (*lo + *hi) / 2.0;
            let start = (mid - min / 2.0).clamp(0.0, 1.0 - min);
            *lo = start;
            *hi = start + min;
        }
        self
    }

    fn into_region(self) -> FocusRegion {
        FocusRegion {
            x: self.x0,
            y: self.y0,
            w: self.x1 - self.x0,
            h: self.y1 - self.y0,
            strength: self.strength,
        }
    }
}

/// `cutoff` 以上のマスを 8 近傍でつなげる。
fn components(scores: &[Option<f32>], cols: usize, rows: usize, cutoff: f32) -> Vec<Component> {
    let on = |x: usize, y: usize| scores[y * cols + x].is_some_and(|s| s >= cutoff);
    let mut seen = vec![false; cols * rows];
    let mut out = Vec::new();
    for sy in 0..rows {
        for sx in 0..cols {
            if seen[sy * cols + sx] || !on(sx, sy) {
                continue;
            }
            let mut c = Component { x0: sx, y0: sy, x1: sx, y1: sy, count: 0, best: 0.0 };
            let mut stack = vec![(sx, sy)];
            seen[sy * cols + sx] = true;
            while let Some((x, y)) = stack.pop() {
                c.count += 1;
                c.best = c.best.max(scores[y * cols + x].unwrap_or(0.0));
                c.x0 = c.x0.min(x);
                c.x1 = c.x1.max(x);
                c.y0 = c.y0.min(y);
                c.y1 = c.y1.max(y);
                for ny in y.saturating_sub(1)..=(y + 1).min(rows - 1) {
                    for nx in x.saturating_sub(1)..=(x + 1).min(cols - 1) {
                        if !seen[ny * cols + nx] && on(nx, ny) {
                            seen[ny * cols + nx] = true;
                            stack.push((nx, ny));
                        }
                    }
                }
            }
            out.push(c);
        }
    }
    out
}

/// 重なる（触れる）四角をひとつにまとめる。強さは強いほう。
fn merge_overlapping(rects: &mut Vec<Rect>) {
    loop {
        let pair = (0..rects.len())
            .flat_map(|i| (i + 1..rects.len()).map(move |j| (i, j)))
            .find(|&(i, j)| rects[i].intersects(&rects[j]));
        let Some((i, j)) = pair else { return };
        let b = rects.remove(j);
        let a = &mut rects[i];
        a.x0 = a.x0.min(b.x0);
        a.y0 = a.y0.min(b.y0);
        a.x1 = a.x1.max(b.x1);
        a.y1 = a.y1.max(b.y1);
        a.strength = a.strength.max(b.strength);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: usize = 512;
    const H: usize = 320;

    /// `period` 画素の升の市松（低 / 高）。
    fn checker(x: usize, y: usize, period: usize, low: f32, high: f32) -> f32 {
        if (x / period + y / period) % 2 == 0 { low } else { high }
    }

    /// ガウスぼかし（σ 画素）。
    fn gaussian(src: &[f32], w: usize, h: usize, sigma: f32) -> Vec<f32> {
        let radius = (sigma * 3.0).ceil() as i32;
        let kernel: Vec<f32> = (-radius..=radius)
            .map(|i| (-((i * i) as f32) / (2.0 * sigma * sigma)).exp())
            .collect();
        let total: f32 = kernel.iter().sum();
        let mut horizontal = vec![0.0; w * h];
        for y in 0..h {
            for x in 0..w {
                let mut sum = 0.0;
                for (i, k) in kernel.iter().enumerate() {
                    let at = (x as i32 + i as i32 - radius).clamp(0, w as i32 - 1) as usize;
                    sum += k * src[y * w + at];
                }
                horizontal[y * w + x] = sum / total;
            }
        }
        let mut out = vec![0.0; w * h];
        for y in 0..h {
            for x in 0..w {
                let mut sum = 0.0;
                for (i, k) in kernel.iter().enumerate() {
                    let at = (y as i32 + i as i32 - radius).clamp(0, h as i32 - 1) as usize;
                    sum += k * horizontal[at * w + x];
                }
                out[y * w + x] = sum / total;
            }
        }
        out
    }

    fn to_u8(values: &[f32]) -> Vec<u8> {
        values.iter().map(|v| v.round().clamp(0.0, 255.0) as u8).collect()
    }

    fn image(mut f: impl FnMut(usize, usize) -> f32) -> Vec<f32> {
        (0..W * H).map(|i| f(i % W, i / W)).collect()
    }

    fn run(values: &[f32]) -> FocusRegions {
        focus_regions(to_u8(values), W as u32, H as u32)
    }

    fn assert_valid(result: &FocusRegions) {
        assert!(result.regions.len() <= MAX_REGIONS);
        for r in &result.regions {
            assert!(r.x >= 0.0 && r.y >= 0.0 && r.w > 0.0 && r.h > 0.0, "{r:?}");
            assert!(r.x + r.w <= 1.0 + 1e-4 && r.y + r.h <= 1.0 + 1e-4, "{r:?}");
            assert!((0.0..=1.0).contains(&r.strength));
        }
    }

    #[test]
    fn 左がくっきり右がぼけた同じ模様なら四角は左だけ() {
        let sharp = image(|x, y| checker(x, y, 12, 40.0, 200.0));
        let soft = gaussian(&sharp, W, H, 2.5);
        let mixed = image(|x, y| if x < W / 2 { sharp[y * W + x] } else { soft[y * W + x] });
        let result = run(&mixed);
        assert_valid(&result);
        assert!(!result.regions.is_empty(), "左に出ていない");
        for r in &result.regions {
            assert!(r.x + r.w <= 0.56, "ぼけた右側に四角が出ている: {r:?}");
        }
    }

    #[test]
    fn 高コントラストのぼけた境目より低コントラストの鋭い模様を選ぶ() {
        // 左: 白黒（0 / 255）を sigma 2.5 でぼかした太い境目。右: 差が 30 しかない鋭い模様。
        let strong = image(|x, y| checker(x, y, 16, 0.0, 255.0));
        let blurred = gaussian(&strong, W, H, 2.5);
        let weak = image(|x, y| checker(x, y, 12, 110.0, 140.0));
        let mixed = image(|x, y| if x < W / 2 { blurred[y * W + x] } else { weak[y * W + x] });
        let result = run(&mixed);
        assert_valid(&result);
        assert!(!result.regions.is_empty(), "鋭い模様が見つかっていない");
        for r in &result.regions {
            assert!(r.x >= 0.44, "ぼけた強い境目のほうに四角が出ている: {r:?}");
        }
    }

    #[test]
    fn 全体がぼけた画像は領域なし() {
        let sharp = image(|x, y| checker(x, y, 16, 20.0, 235.0));
        for sigma in [2.0, 3.0] {
            let result = run(&gaussian(&sharp, W, H, sigma));
            assert!(result.regions.is_empty(), "sigma {sigma}: {:?}", result.regions);
        }
    }

    #[test]
    fn のっぺりやノイズだけの画像は領域なし() {
        assert!(run(&image(|_, _| 128.0)).regions.is_empty());
        assert!(run(&image(|x, _| 60.0 + x as f32 * 0.2)).regions.is_empty()); // なだらかな階調
        // 振れ幅 ±12 の疑似ノイズ（LCG）。
        let mut state = 12345u32;
        let noise = image(|_, _| {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            128.0 + ((state >> 16) % 25) as f32 - 12.0
        });
        assert!(run(&noise).regions.is_empty());
    }

    #[test]
    fn 全面がくっきりなら大きな四角が1個() {
        let result = run(&image(|x, y| checker(x, y, 8, 20.0, 235.0)));
        assert_valid(&result);
        assert_eq!(result.regions.len(), 1);
        let r = &result.regions[0];
        assert!(r.w > 0.9 && r.h > 0.9, "{r:?}");
    }

    #[test]
    fn 離れた鋭い所が5つあっても四角は3個まで_強い順() {
        // のっぺりした地に、鋭い模様の小片を 5 つ。
        let patches = [(40, 40), (240, 40), (440, 40), (140, 220), (340, 220)];
        let values = image(|x, y| {
            for (i, &(px, py)) in patches.iter().enumerate() {
                if x >= px && x < px + 64 && y >= py && y < py + 64 {
                    // 片ごとに差を変えて、強さに順番をつける。
                    let swing = 220.0 - 30.0 * i as f32;
                    return checker(x - px, y - py, 8, 128.0 - swing / 2.0, 128.0 + swing / 2.0);
                }
            }
            128.0
        });
        let result = run(&values);
        assert_valid(&result);
        assert_eq!(result.regions.len(), MAX_REGIONS);
        for pair in result.regions.windows(2) {
            assert!(pair[0].strength >= pair[1].strength);
        }
    }

    #[test]
    fn 小さな領域は最小の大きさに広げる() {
        let values = image(|x, y| {
            if (200..232).contains(&x) && (100..132).contains(&y) { checker(x, y, 4, 20.0, 235.0) } else { 128.0 }
        });
        let result = run(&values);
        assert_valid(&result);
        assert_eq!(result.regions.len(), 1, "{:?}", result.regions);
        let r = &result.regions[0];
        let min = MIN_REGION_SIDE * W as f32;
        assert!(r.w * W as f32 >= min - 1.0, "{r:?}");
        assert!(r.h * H as f32 >= min - 1.0, "{r:?}");
    }

    #[test]
    fn 読めない入力でも落ちない() {
        for (len, w, h) in [(0, 0, 0), (0, 10, 0), (100, 0, 100), (4, 2, 2), (50, 10, 10), (10, 10, 1), (9, 3, 3)] {
            let result = focus_regions(vec![200; len], w, h);
            assert!(result.regions.is_empty());
        }
        assert!(focus_regions(vec![1; 5], 3, 3).regions.is_empty());
        assert!(focus_regions(vec![], u32::MAX, u32::MAX).regions.is_empty());
    }

    #[test]
    fn 長辺2048でも動く() {
        let (w, h) = (2048usize, 1365usize);
        let gray: Vec<u8> = (0..w * h)
            .map(|i| if ((i % w) / 8 + (i / w) / 8) % 2 == 0 { 20 } else { 235 })
            .collect();
        let result = focus_regions(gray, w as u32, h as u32);
        assert_valid(&result);
        assert_eq!(result.regions.len(), 1);
    }
}
