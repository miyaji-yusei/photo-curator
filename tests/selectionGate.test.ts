import { describe, expect, it } from 'vitest'
import { canStartSelection } from '../utils/selectionGate'
import type { SelectionGateFacts } from '../utils/selectionGate'

// 各入口が今どの条件を見ているか（`useCurator` の旧ガードと `ProjectView` の旧 :disabled をそのまま写した）。
const clear = { photoCount: 10, sidecarClash: false, sidecarChecking: false, hasProject: true, taskDialog: false }
const entries: Record<string, { reads: (keyof SelectionGateFacts)[], blockedBy: (keyof SelectionGateFacts)[] }> = {
  enterMethod: { reads: ['photoCount', 'sidecarClash', 'sidecarChecking'], blockedBy: ['photoCount', 'sidecarClash', 'sidecarChecking'] },
  startButton: { reads: ['photoCount', 'sidecarClash', 'sidecarChecking'], blockedBy: ['photoCount', 'sidecarClash', 'sidecarChecking'] },
  beginTournament: { reads: ['hasProject', 'taskDialog', 'sidecarClash'], blockedBy: ['hasProject', 'taskDialog', 'sidecarClash'] },
  startTournament: { reads: ['hasProject', 'taskDialog', 'sidecarClash'], blockedBy: ['hasProject', 'taskDialog', 'sidecarClash'] },
  resumeSession: { reads: ['sidecarClash', 'sidecarChecking'], blockedBy: ['sidecarClash', 'sidecarChecking'] }
}
const bad: Record<string, unknown> = { photoCount: 0, sidecarClash: true, sidecarChecking: true, hasProject: false, taskDialog: true }
const all = Object.keys(bad) as (keyof SelectionGateFacts)[]

function factsFor(entry: string, overrides: Record<string, unknown>): SelectionGateFacts {
  const facts: Record<string, unknown> = {}
  for (const key of entries[entry].reads) facts[key] = (clear as Record<string, unknown>)[key]
  return { ...facts, ...overrides } as SelectionGateFacts
}

describe('canStartSelection（入口 × 条件）', () => {
  for (const entry of Object.keys(entries)) {
    it(`${entry}: 何も引っかからなければ始められる`, () => {
      expect(canStartSelection(factsFor(entry, {}))).toBe(true)
    })
    for (const key of all) {
      const blocks = entries[entry].blockedBy.includes(key)
      it(`${entry}: ${key} が悪いと${blocks ? '始められない' : 'ここでは見ないので始められる'}`, () => {
        // 見ない条件は、その入口が事実を渡さない（＝判定に出てこない）ので、渡さずに確かめる。
        const facts = blocks ? factsFor(entry, { [key]: bad[key] }) : factsFor(entry, {})
        expect(canStartSelection(facts)).toBe(!blocks)
      })
    }
  }

  it('事実を渡さなければ（何も見ていない）始められる', () => {
    expect(canStartSelection({})).toBe(true)
  })

  it('photoCount が未定（0 と同じ扱い）なら始められない', () => {
    expect(canStartSelection({ photoCount: Number.NaN })).toBe(false)
  })

  it('走査中は条件に無い（B9。直すときはここに 1 行足す）', () => {
    expect(Object.keys(clear)).not.toContain('scanRunning')
  })
})
