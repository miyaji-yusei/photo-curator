import { afterEach, describe, expect, it, vi } from 'vitest'
import { randomUUID } from '~/utils/uuid'

const V4 = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/

describe('randomUUID', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('crypto.randomUUID が無い環境（http の LAN）でも v4 の形で返す', () => {
    vi.stubGlobal('crypto', {
      getRandomValues: (bytes: Uint8Array) => {
        for (let index = 0; index < bytes.length; index += 1) bytes[index] = (index * 37 + 11) & 0xff
        return bytes
      }
    })
    expect(randomUUID()).toMatch(V4)
  })

  it('crypto.randomUUID があればそれを使う', () => {
    vi.stubGlobal('crypto', { randomUUID: () => 'fixed' })
    expect(randomUUID()).toBe('fixed')
  })
})
