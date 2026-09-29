import { describe, expect, it } from 'vitest'
import type { Session } from '~/lib/core'
import {
  applyChanges, clampStar, moveRatings, pathsWithRating, reviewChanges, setRating
} from '~/utils/ratingEdit'

function session(ratings: Record<string, number>, targetStar = 1): Session {
  return {
    group_size: 4, target_star: targetStar, round: 1, queue: [], current: [], survivors: [],
    ratings, members: {}, history: [], finished: false
  }
}

describe('clampStar', () => {
  it('0..5 に丸める', () => {
    expect(clampStar(-3)).toBe(0)
    expect(clampStar(9)).toBe(5)
    expect(clampStar(2.6)).toBe(3)
    expect(clampStar(Number.NaN)).toBe(0)
  })
})

describe('setRating', () => {
  it('その 1 枚だけを変え、元の Session は変えない', () => {
    const before = session({ a: 1, b: 1 })
    const after = setRating(before, 'a', 5)
    expect(after.ratings).toEqual({ a: 5, b: 1 })
    expect(before.ratings).toEqual({ a: 1, b: 1 })
  })

  it('範囲外は 0..5 に丸める', () => {
    expect(setRating(session({ a: 1 }), 'a', 7).ratings.a).toBe(5)
    expect(setRating(session({ a: 1 }), 'a', -1).ratings.a).toBe(0)
  })
})

describe('applyChanges', () => {
  it('絶対値で書き、触らない写真はそのまま', () => {
    const next = applyChanges(session({ a: 1, b: 2, c: 3 }), { a: 4, c: 9 })
    expect(next.ratings).toEqual({ a: 4, b: 2, c: 5 })
  })

  it('Session 以外のフィールドは変えない', () => {
    const before = session({ a: 1 })
    const next = applyChanges(before, { a: 2 })
    expect({ ...next, ratings: before.ratings }).toEqual(before)
  })
})

describe('reviewChanges（連写の見直し）', () => {
  it('残す写真は +1、外した写真は −1、0..5 に丸める', () => {
    const base = session({ a: 2, b: 2, c: 0, d: 5 })
    const changes = reviewChanges(base, ['a', 'b', 'c', 'd'], ['a', 'd'])
    expect(changes).toEqual({ a: 3, b: 1, c: 0, d: 5 })
  })

  it('applyChanges に渡すと星が動く', () => {
    const base = session({ a: 2, b: 2 })
    const next = applyChanges(base, reviewChanges(base, ['a', 'b'], ['b']))
    expect(next.ratings).toEqual({ a: 1, b: 3 })
  })

  it('Session に無い写真は 0 から数える', () => {
    expect(reviewChanges(session({}), ['x'], ['x'])).toEqual({ x: 1 })
  })
})

describe('moveRatings（星の一括移動）', () => {
  it('Session に無い写真は飛ばす', () => {
    const next = moveRatings(session({ a: 1, b: 1 }), ['a', 'zzz'], 3)
    expect(next.ratings).toEqual({ a: 3, b: 1 })
    expect('zzz' in next.ratings).toBe(false)
  })

  it('移動先も 0..5 に丸める', () => {
    expect(moveRatings(session({ a: 1 }), ['a'], 8).ratings.a).toBe(5)
  })
})

describe('pathsWithRating', () => {
  it('その星ちょうどの写真だけ', () => {
    expect(pathsWithRating(session({ a: 1, b: 2, c: 1 }), 1).sort()).toEqual(['a', 'c'])
  })
})
