import { describe, expect, it } from 'vitest'
import { canStartSelection } from '../utils/selectionGate'
import type { SelectionGateFacts } from '../utils/selectionGate'

// 各入口が今どの条件を見ているか（`useCurator` の旧ガードと `ProjectView` の旧 :disabled をそのまま写した）。
const clear = { photoCount: 10, sidecarClash: false, sidecarChecking: false, hasProject: true, taskDialog: false, scanRunning: false }
const entries: Record<string, { reads: (keyof SelectionGateFacts)[], blockedBy: (keyof SelectionGateFacts)[] }> = {
  enterMethod: { reads: ['photoCount', 'scanRunning', 'sidecarClash', 'sidecarChecking'], blockedBy: ['photoCount', 'scanRunning', 'sidecarClash', 'sidecarChecking'] },
  startButton: { reads: ['photoCount', 'scanRunning', 'sidecarClash', 'sidecarChecking'], blockedBy: ['photoCount', 'scanRunning', 'sidecarClash', 'sidecarChecking'] },
  beginTournament: { reads: ['hasProject', 'taskDialog', 'scanRunning', 'sidecarClash'], blockedBy: ['hasProject', 'taskDialog', 'scanRunning', 'sidecarClash'] },
  startTournament: { reads: ['hasProject', 'taskDialog', 'scanRunning', 'sidecarClash'], blockedBy: ['hasProject', 'taskDialog', 'scanRunning', 'sidecarClash'] },
  resumeSession: { reads: ['scanRunning', 'sidecarClash', 'sidecarChecking'], blockedBy: ['scanRunning', 'sidecarClash', 'sidecarChecking'] }
}
const bad: Record<string, unknown> = { photoCount: 0, sidecarClash: true, sidecarChecking: true, hasProject: false, taskDialog: true, scanRunning: true }
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

  it('走査中は始められない（B9。どの入口も見る）', () => {
    expect(canStartSelection({ scanRunning: true })).toBe(false)
    expect(canStartSelection({ scanRunning: false })).toBe(true)
  })
})
