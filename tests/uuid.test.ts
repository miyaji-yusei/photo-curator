import { afterEach, describe, expect, it } from 'vitest'
import { randomUUID } from '~/utils/uuid'

const UUID_V4 = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/

describe('randomUUID', () => {
  const original = crypto.randomUUID
  afterEach(() => { crypto.randomUUID = original })

  it('crypto.randomUUID があればそれを使う', () => {
    expect(UUID_V4.test(randomUUID())).toBe(true)
  })

  it('secure context でなく crypto.randomUUID が無くても作れる（LAN の http:// アクセス）', () => {
    // @ts-expect-error -- 実機（http:// で LAN からアクセス）を模す
    crypto.randomUUID = undefined
    const id = randomUUID()
    expect(UUID_V4.test(id)).toBe(true)
  })

  it('毎回違う値になる', () => {
    // @ts-expect-error -- 上と同じく無い場合を模す
    crypto.randomUUID = undefined
    expect(randomUUID()).not.toBe(randomUUID())
  })
})
