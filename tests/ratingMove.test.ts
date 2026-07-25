import { describe, expect, it } from 'vitest'
import {
  createMoveSelection, isSelected, selectedCount, setSelectAll, toMoveArgs, toggleSelection
} from '~/utils/ratingMove'

describe('レート移動の選択状態', () => {
  it('既定は全選択。まだ読み込んでいない写真も対象に入る', () => {
    const selection = createMoveSelection()
    expect(isSelected(selection, 'まだ読み込んでいない写真')).toBe(true)
    // 表示中が 80 枚でも、対象は総数の 5,000 枚。
    expect(selectedCount(selection, 5000)).toBe(5000)
  })

  it('全選択から外した写真だけが対象から抜ける', () => {
    const selection = toggleSelection(createMoveSelection(), 'p2')
    expect(isSelected(selection, 'p1')).toBe(true)
    expect(isSelected(selection, 'p2')).toBe(false)
    expect(selectedCount(selection, 5000)).toBe(4999)
  })

  it('全解除すると、チェックした写真だけが対象になる', () => {
    let selection = setSelectAll(false)
    expect(selectedCount(selection, 5000)).toBe(0)

    selection = toggleSelection(selection, 'p1')
    expect(isSelected(selection, 'p1')).toBe(true)
    expect(isSelected(selection, 'p2')).toBe(false)
    expect(selectedCount(selection, 5000)).toBe(1)
  })

  it('同じ写真を2回押すと基準に戻る', () => {
    const selection = toggleSelection(toggleSelection(createMoveSelection(), 'p1'), 'p1')
    expect(isSelected(selection, 'p1')).toBe(true)
    expect(selection.deviations.size).toBe(0)
  })

  it('全選択・全解除を押すと、それまでの差分は捨てられる', () => {
    const touched = toggleSelection(createMoveSelection(), 'p1')
    expect(setSelectAll(true).deviations.size).toBe(0)
    expect(setSelectAll(false).deviations.size).toBe(0)
    expect(touched.deviations.size).toBe(1) // 元は変えない
  })

  it('全選択なら「除外」で送る。5,000 件の id を送らずに済む', () => {
    const selection = toggleSelection(createMoveSelection(), 'p2')
    expect(toMoveArgs(selection)).toEqual({ includeIds: null, excludeIds: ['p2'] })
  })

  it('全解除からの積み上げなら「明示指定」で送る', () => {
    const selection = toggleSelection(setSelectAll(false), 'p1')
    expect(toMoveArgs(selection)).toEqual({ includeIds: ['p1'], excludeIds: [] })
  })

  it('全選択のまま何も外さなければ、id を1件も送らない', () => {
    // include が null なので、サーバ側は WHERE rating=? だけで全件を動かす。
    expect(toMoveArgs(createMoveSelection())).toEqual({ includeIds: null, excludeIds: [] })
  })
})
