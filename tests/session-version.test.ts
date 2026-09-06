import { describe, expect, it } from 'vitest'
import { isCurrentSession, makeSession, SESSION_VERSION } from '~/utils/tournament'
import type { SelectionSession } from '~/types/photo'

const settings = { groupSize: 4, groupBursts: false }

describe('セッションの版', () => {
  /**
   * **形を変えたら、それ以前の保存は読まない。**
   *
   * 星もサムネイルも写真の側に持っているので、捨てて困るのは「いまどこまで
   * 選んだか」だけ。移行を書くより確実で、失うものも小さい。
   */
  it('新しく作ったセッションは今の版を名乗る', () => {
    const session = makeSession('p', [{ id: 'a', rating: 0, capturedAt: 1 }], settings)
    expect(session.version).toBe(SESSION_VERSION)
    expect(isCurrentSession(session)).toBe(true)
  })

  it('版の無い保存は読まない', () => {
    const old = { projectId: 'p' } as SelectionSession
    expect(isCurrentSession(old)).toBe(false)
  })

  it('版が古い保存は読まない', () => {
    const old = { projectId: 'p', version: SESSION_VERSION - 1 } as SelectionSession
    expect(isCurrentSession(old)).toBe(false)
  })

  it('無い（開いたことがない）ときも読まない扱い', () => {
    expect(isCurrentSession(null)).toBe(false)
  })
})
