// @vitest-environment node
import { computed, effectScope, ref, watchEffect, nextTick } from 'vue'
import { describe, expect, it } from 'vitest'
import { firstRow, lastRow, type RangeInput } from '~/utils/virtualRange'

const input = (over: Partial<RangeInput> = {}): RangeInput =>
  ({ rows: 100, scrolled: 0, viewport: 800, stride: 100, overscan: 2, ...over })

describe('firstRow / lastRow', () => {
  it('先頭・途中・末尾・空', () => {
    expect([firstRow(input()), lastRow(input())]).toEqual([0, 10])
    expect([firstRow(input({ scrolled: 1000 })), lastRow(input({ scrolled: 1000 }))]).toEqual([8, 20])
    expect([firstRow(input({ scrolled: 99999 })), lastRow(input({ scrolled: 99999 }))]).toEqual([97, 99])
    expect([firstRow(input({ rows: 0 })), lastRow(input({ rows: 0 }))]).toEqual([0, -1])
    expect(firstRow(input({ scrolled: -500 }))).toBe(0)
  })
})

/** 格子の描画に当たる処理が、スクロール 600 フレームで何回走るか（同じ行にとどまるフレームが大半）。 */
async function renders(split: boolean) {
  const scrolled = ref(0)
  const scope = effectScope()
  let count = 0
  scope.run(() => {
    const range = computed(() => {
      const i = input({ scrolled: scrolled.value })
      return { first: firstRow(i), last: lastRow(i) }
    })
    const first = computed(() => firstRow(input({ scrolled: scrolled.value })))
    const last = computed(() => lastRow(input({ scrolled: scrolled.value })))
    watchEffect(() => {
      if (split) void [first.value, last.value]
      else void [range.value.first, range.value.last]
      count += 1
    }, { flush: 'sync' })
  })
  // 1 フレーム 3px ずつ、600 フレーム（行の高さ 100+14 に対して約 16 行ぶん進む）。
  for (let frame = 1; frame <= 600; frame += 1) {
    scrolled.value = frame * 3
    await nextTick()
  }
  scope.stop()
  return count
}

describe('W14: 行が変わらないフレームでは再描画しない', () => {
  it('first・last を別の computed にすると、再描画の回数が行の変わった回数まで減る', async () => {
    const before = await renders(false)
    const after = await renders(true)
    console.log(`[bench] スクロール 600 フレームの再描画: ${before} 回 → ${after} 回`)
    expect(before).toBe(601)
    expect(after).toBeLessThan(40)
  })
})
