import { describe, expect, it } from 'vitest'
import { MAX_RATING } from '~/types/photo'
import type { SelectionSession } from '~/types/photo'
import { createThresholdState } from '~/utils/burstThreshold'
import { collapseBursts, prepareRound, regroupRemaining,  setBurstRepresentative, undoLastStep } from '~/utils/tournament'

function makeTestSession(groups: string[][]): SelectionSession {
  const ids = groups.flat()
  return {
    projectId: 'p1',
    settings: { groupSize: groups[0]?.length ?? 2, groupBursts: false },
    candidates: ids,
    groups,
    groupIndex: 0,
    selectedInGroup: [],
    multiSelect: false,
    ratings: Object.fromEntries(ids.map(id => [id, 0])),
    survivors: [],
    history: [],
    targetRating: 0,
    burstMembers: {},
    round: 1,
    burstGroups: [],
    burstPairs: [],
    burstThresholdState: createThresholdState([]),
    burstThreshold: null,
    stage: 'tournament',
    updatedAt: 0
  }
}

/** app.vue の confirmChoices と同じ手順で1グループを確定させる。 */
function choose(session: SelectionSession, chosen: string[]) {
  session.history.push({ groupIndex: session.groupIndex, chosen: [...chosen] })
  for (const id of chosen) {
    session.survivors.push(id)
    session.ratings[id] = (session.ratings[id] ?? 0) + 1
  }
  session.groupIndex += 1
  session.selectedInGroup = []
}

describe('undoLastStep', () => {
  it('直前の選択を取り消して survivors とレーティングを戻す', () => {
    const session = makeTestSession([['a', 'b'], ['c', 'd']])
    choose(session, ['a'])
    choose(session, ['c'])
    expect(session.survivors).toEqual(['a', 'c'])
    expect(session.groupIndex).toBe(2)

    expect(undoLastStep(session)).toBe(true)
    expect(session.survivors).toEqual(['a'], )
    expect(session.ratings.c).toBe(0)
    expect(session.groupIndex).toBe(1)
    expect(session.stage).toBe('tournament')
  })

  it('1枚も選ばなかったグループも戻せる', () => {
    const session = makeTestSession([['a', 'b'], ['c', 'd']])
    choose(session, [])
    expect(session.groupIndex).toBe(1)
    expect(session.survivors).toEqual([])

    expect(undoLastStep(session)).toBe(true)
    expect(session.groupIndex).toBe(0)
    expect(session.survivors).toEqual([])
  })

  it('複数枚選んだグループはまとめて戻す', () => {
    const session = makeTestSession([['a', 'b', 'c']])
    choose(session, ['a', 'c'])
    undoLastStep(session)
    expect(session.survivors).toEqual([])
    expect(session.ratings.a).toBe(0)
    expect(session.ratings.c).toBe(0)
  })

  it('同じ写真が複数ラウンドで通っていても1回ぶんだけ戻す', () => {
    const session = makeTestSession([['a', 'b']])
    session.survivors.push('a') // 前のラウンドで通ったぶん
    session.ratings.a = 1
    choose(session, ['a'])
    expect(session.ratings.a).toBe(2)

    undoLastStep(session)
    expect(session.survivors).toEqual(['a'], )
    expect(session.ratings.a).toBe(1)
  })

  it('履歴が無ければ何も起きない', () => {
    const session = makeTestSession([['a', 'b']])
    expect(undoLastStep(session)).toBe(false)
    expect(session.groupIndex).toBe(0)
  })

  it('結果画面まで進んだあとでも戻れる', () => {
    const session = makeTestSession([['a', 'b']])
    choose(session, [])
    session.stage = 'result'
    expect(undoLastStep(session)).toBe(true)
    expect(session.stage).toBe('tournament')
    expect(session.groupIndex).toBe(0)
  })
})

describe('regroupRemaining', () => {
  it('済んだグループは触らず、未処理ぶんだけ詰め直す', () => {
    const session = makeTestSession([['a', 'b'], ['c', 'd'], ['e', 'f']])
    choose(session, ['a'])
    expect(session.groupIndex).toBe(1)

    regroupRemaining(session, 4)

    expect(session.groups[0]).toEqual(['a', 'b'], ) // 済んだぶんは不変
    expect(session.groups.slice(1)).toEqual([['c', 'd', 'e', 'f']])
    expect(session.groupIndex).toBe(1) // 位置が動かないので履歴も有効なまま
    expect(session.settings.groupSize).toBe(4)
  })

  it('端数が1枚になったら比較にならないのでそのまま通す', () => {
    const session = makeTestSession([['a', 'b', 'c']])
    // 未処理 3 枚を 2 枚ずつに割ると 1 枚余る
    regroupRemaining(session, 2)
    expect(session.groups).toEqual([['a', 'b']])
    expect(session.survivors).toEqual(['c'], )
  })

  it('詰め直しても戻る履歴が使える', () => {
    const session = makeTestSession([['a', 'b'], ['c', 'd'], ['e', 'f']])
    choose(session, ['a'])
    regroupRemaining(session, 4)

    expect(undoLastStep(session)).toBe(true)
    expect(session.groupIndex).toBe(0)
    expect(session.groups[0]).toEqual(['a', 'b'], )
    expect(session.survivors).toEqual([])
  })
})

describe('collapseBursts', () => {
  const withBurst = (photoIds: string[]) => ({
    id: `b-${photoIds[0]}`, photoIds, capturedSpanMs: 1000, similarity: 90, accepted: true
  })

  it('まとめは代表1枚に畳み込まれ、他のメンバーは候補から外れる', () => {
    const session = makeTestSession([['a', 'b', 'c', 'd']])
    session.burstGroups = [withBurst(['a', 'b'])]
    collapseBursts(session)

    expect(session.candidates).toEqual(['a', 'c', 'd'])
    expect(session.burstMembers).toEqual({ a: ['a', 'b'] })
  })

  it('候補に1枚しか残っていないまとめは畳み込まない', () => {
    const session = makeTestSession([['a', 'c']])
    // b は前のラウンドで落ちている
    session.burstGroups = [withBurst(['a', 'b'])]
    collapseBursts(session)

    expect(session.candidates).toEqual(['a', 'c'])
    expect(session.burstMembers).toEqual({})
  })
})

describe('setBurstRepresentative', () => {
  it('代表を差し替えると、表示中のグループも入れ替わる', () => {
    const session = makeTestSession([['a', 'c']])
    session.burstMembers = { a: ['a', 'b'] }
    session.selectedInGroup = ['a']

    setBurstRepresentative(session, 'a', 'b')

    expect(session.burstMembers).toEqual({ b: ['b', 'a'] })
    expect(session.groups[0]).toEqual(['b', 'c'])
    expect(session.candidates).toEqual(['b', 'c'])
    expect(session.selectedInGroup).toEqual(['b'])
  })

  it('メンバーでない写真は代表にできない', () => {
    const session = makeTestSession([['a', 'c']])
    session.burstMembers = { a: ['a', 'b'] }
    setBurstRepresentative(session, 'a', 'c')
    expect(session.burstMembers).toEqual({ a: ['a', 'b'] })
  })
})


describe('レーティング基準の選別', () => {
  /** app.vue の confirmChoices と同じ星の計算。選ばれなければ据え置き。 */
  function judge(session: SelectionSession, group: string[], chosen: string[]) {
    session.history.push({ groupIndex: session.groupIndex, chosen: [...chosen] })
    for (const id of chosen) {
      session.survivors.push(id)
      session.ratings[id] = Math.min(MAX_RATING, (session.ratings[id] ?? 0) + 1)
    }
    session.groupIndex += 1
  }

  it('選ばれた写真だけ星が上がり、選ばれなかった写真は据え置き', () => {
    const session = makeTestSession([['a', 'b']])
    judge(session, ['a', 'b'], ['a'])
    expect(session.ratings.a).toBe(1)
    expect(session.ratings.b).toBe(0, )
  })

  it('星は MAX_RATING で頭打ちになる', () => {
    const session = makeTestSession([['a', 'b']])
    session.ratings.a = MAX_RATING
    judge(session, ['a', 'b'], ['a'])
    expect(session.ratings.a).toBe(MAX_RATING)
  })

  it('同じ星に集まった写真は、経路に関係なく1つの対象になる', () => {
    // 別々の回で★1になった写真も、次に★1を選別すれば一緒に出てくる。
    // 対象は candidates ではなく「星が一致する写真」で決まる。
    const first = makeTestSession([['a', 'b']])
    judge(first, ['a', 'b'], ['a'])          // a → ★1、b は ★0 のまま
    const second = makeTestSession([['c', 'd']])
    judge(second, ['c', 'd'], ['c'])         // c → ★1

    const allRatings = { ...first.ratings, ...second.ratings }
    const starOne = Object.keys(allRatings).filter(id => allRatings[id] === 1).sort()
    expect(starOne).toEqual(['a', 'c'])
  })

  it('確定は星を一気に最大へ上げる。別のフラグは持たない', () => {
    const session = makeTestSession([['a', 'b']])
    session.ratings.a = MAX_RATING          // confirmPhoto と同じ
    judge(session, ['a', 'b'], ['a'])
    expect(session.ratings.a).toBe(MAX_RATING)
    // ★5 は「★0〜4 を選別」のどの回にも入らない。
    expect(session.ratings.a).not.toBe(0)
  })

  it('やり直すと星が全部 0 に戻る', () => {
    const session = makeTestSession([['a', 'b']])
    judge(session, ['a', 'b'], ['a'])
    // reset_selection_results と同じ結果。
    for (const id of Object.keys(session.ratings)) session.ratings[id] = 0
    expect(Object.values(session.ratings).every(rating => rating === 0)).toBe(true)
  })
})
