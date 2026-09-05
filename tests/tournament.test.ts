import { describe, expect, it } from 'vitest'
import { MAX_RATING } from '~/types/photo'
import type { SelectionSession } from '~/types/photo'
import { createThresholdState } from '~/utils/burstThreshold'
import { collapseBursts, insertIntoUpcoming, prepareRound, regroupRemaining, resolveChosen, reviewBurstRatings, setBurstRepresentative, spreadBurstRatings, undoLastStep } from '~/utils/tournament'

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
    burstSettled: [],
    round: 1,
    burstGroups: [],
    burstPairs: [],
    burstThresholdState: createThresholdState([]),
    burstThreshold: null,
    stage: 'tournament',
    updatedAt: 0
  }
}

/** 撮影順に並んだ候補だけを持つ、prepareRound に掛ける前のセッション。 */
function makeSeededSession(candidates: string[], groupSize: number): SelectionSession {
  const session = makeTestSession([])
  session.candidates = [...candidates]
  session.ratings = Object.fromEntries(candidates.map(id => [id, 0]))
  session.settings = { groupSize, groupBursts: false }
  return session
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

describe('prepareRound の並び順', () => {
  // 似た構図は撮影時刻が近い。get_selection_seed が撮影順で返す並びを
  // prepareRound が崩すと、同じ場面の写真が別々のグループにばらけて
  // 「その中の1枚を選ぶ」比較にならない。
  it('候補を並べ替えず、渡された順のままグループに切る', () => {
    const order = ['p1', 'p2', 'p3', 'p4', 'p5', 'p6']
    const session = makeSeededSession(order, 3)

    prepareRound(session)

    expect(session.groups).toEqual([['p1', 'p2', 'p3'], ['p4', 'p5', 'p6']])
  })

  it('端数が1枚になったら比較にならないのでそのまま通す', () => {
    const session = makeSeededSession(['p1', 'p2', 'p3'], 2)
    prepareRound(session)
    expect(session.groups).toEqual([['p1', 'p2']])
    expect(session.survivors).toEqual(['p3'])
  })

  it('2ラウンド目も撮影順が保たれる', () => {
    const session = makeSeededSession(['p1', 'p2', 'p3', 'p4', 'p5', 'p6'], 2)
    prepareRound(session)
    // 各グループの先頭を通す。survivors はグループ順に積まれる。
    for (const group of session.groups) choose(session, [group[0]!])
    session.candidates = [...new Set(session.survivors)]
    session.survivors = []

    prepareRound(session)

    expect(session.candidates).toEqual(['p1', 'p3', 'p5'])
    expect(session.groups).toEqual([['p1', 'p3']])
    expect(session.survivors).toEqual(['p5'])
  })

  it('まとめを畳み込んでも撮影順が崩れない', () => {
    const session = makeSeededSession(['p1', 'p2', 'p3', 'p4'], 2)
    // p2 と p3 が連写。代表は p2。
    session.burstGroups = [
      { id: 'b1', photoIds: ['p2', 'p3'], capturedSpanMs: 500, similarity: 95, accepted: true }
    ]
    collapseBursts(session)
    prepareRound(session)

    expect(session.candidates).toEqual(['p1', 'p2', 'p4'])
    expect(session.groups).toEqual([['p1', 'p2']])
    expect(session.survivors).toEqual(['p4'])
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


describe('resolveChosen（確定と選択の合流）', () => {
  it('選択した写真をそのまま通す', () => {
    expect(resolveChosen(['a'], ['a', 'b', 'c'], { a: 2, b: 2, c: 2 }, 5)).toEqual(['a'])
  })

  it('★5に確定した写真は、選んでいなくても通る', () => {
    // 複数枚選択中に b を★5トグルしたが、決定時の selectedInGroup には無い。
    const chosen = resolveChosen(['a'], ['a', 'b', 'c'], { a: 2, b: 5, c: 2 }, 5)
    expect(chosen).toEqual(['a', 'b'])
  })

  it('「選択なしで次へ」でも、確定した写真は残る', () => {
    // これが今回の不具合の核。確定してから何も選ばず決定しても落とさない。
    const chosen = resolveChosen([], ['a', 'b'], { a: 5, b: 2 }, 5)
    expect(chosen).toEqual(['a'])
  })

  it('選択と確定が重なっても二重に数えない', () => {
    const chosen = resolveChosen(['a', 'b'], ['a', 'b'], { a: 5, b: 2 }, 5)
    expect(chosen).toEqual(['a', 'b'])
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

// ---------------------------------------------------------------------------
// Routine 6: 連写の事後扱い
// ---------------------------------------------------------------------------

describe('spreadBurstRatings', () => {
  /** 代表 a がまとめている 3 枚と、まとめに属さない b。 */
  const withBurst = () => {
    const session = makeTestSession([['a', 'b']])
    session.burstMembers = { a: ['a', 'a2', 'a3'] }
    session.ratings = { a: 0, a2: 0, a3: 0, b: 0 }
    return session
  }

  it('代表と同じ星を仲間にも配り、配った id を返す', () => {
    const session = withBurst()
    session.ratings.a = 3
    expect(spreadBurstRatings(session, ['a'])).toEqual(['a2', 'a3'])
    expect(session.ratings.a2).toBe(3)
    expect(session.ratings.a3).toBe(3)
  })

  it('★5で確定した代表にも追随する。+1 では追いつかない', () => {
    const session = withBurst()
    session.ratings.a = MAX_RATING
    spreadBurstRatings(session, ['a'])
    expect(session.ratings.a2).toBe(MAX_RATING)
  })

  it('通らなかった代表のまとめには手を付けない', () => {
    const session = withBurst()
    session.ratings.a = 3
    expect(spreadBurstRatings(session, ['b'])).toEqual([])
    expect(session.ratings.a2).toBe(0)
  })

  it('まとめを持たない写真は何も起きない', () => {
    const session = withBurst()
    session.ratings.b = 4
    expect(spreadBurstRatings(session, ['b'])).toEqual([])
    expect(session.ratings.b).toBe(4)
  })

  it('代表自身は配る対象に含めない（二重に数えない）', () => {
    const session = withBurst()
    session.ratings.a = 2
    expect(spreadBurstRatings(session, ['a'])).not.toContain('a')
  })
})

describe('reviewBurstRatings', () => {
  const photos = [
    { id: 'a', rating: 3 },
    { id: 'b', rating: 3 },
    { id: 'c', rating: 3 }
  ]

  it('残した写真は上がり、外した写真は下がる', () => {
    expect(reviewBurstRatings(photos, ['b'], MAX_RATING)).toEqual([
      { id: 'a', rating: 2 },
      { id: 'b', rating: 4 },
      { id: 'c', rating: 2 }
    ])
  })

  it('★5 を超えて上がらない', () => {
    expect(reviewBurstRatings([{ id: 'a', rating: MAX_RATING }], ['a'], MAX_RATING))
      .toEqual([{ id: 'a', rating: MAX_RATING }])
  })

  it('★0 を下回って下がらない', () => {
    expect(reviewBurstRatings([{ id: 'a', rating: 0 }], [], MAX_RATING))
      .toEqual([{ id: 'a', rating: 0 }])
  })

  it('複数枚を残せる', () => {
    const result = reviewBurstRatings(photos, ['a', 'c'], MAX_RATING)
    expect(result.map(entry => entry.rating)).toEqual([4, 2, 4])
  })

  it('まとめに含まれない id を残す指定は無視する', () => {
    expect(reviewBurstRatings([{ id: 'a', rating: 3 }], ['z'], MAX_RATING))
      .toEqual([{ id: 'a', rating: 2 }])
  })
})

// 代表と仲間で星が食い違ったままになると、結果一覧に説明できない星が残る。
// app.vue の confirmChoices / undoChoice と同じ順序で確かめる。
describe('連写の星は代表と一緒に動く', () => {
  const session = () => {
    const built = makeTestSession([['a', 'b']])
    built.burstMembers = { a: ['a', 'a2'] }
    built.ratings = { a: 2, a2: 2, b: 2 }
    built.targetRating = 2
    return built
  }

  it('代表が通ると仲間も同じ星に上がる', () => {
    const current = session()
    const chosen = resolveChosen(['a'], ['a', 'b'], current.ratings, MAX_RATING)
    for (const id of chosen) {
      current.survivors.push(id)
      current.ratings[id] = Math.min(MAX_RATING, (current.ratings[id] ?? 0) + 1)
    }
    spreadBurstRatings(current, chosen)
    expect(current.ratings.a).toBe(3)
    expect(current.ratings.a2).toBe(3)
    expect(current.ratings.b).toBe(2)
  })

  it('1つ戻すと仲間の星も元に戻る', () => {
    const current = session()
    const chosen = resolveChosen(['a'], ['a', 'b'], current.ratings, MAX_RATING)
    for (const id of chosen) {
      current.survivors.push(id)
      current.ratings[id] = Math.min(MAX_RATING, (current.ratings[id] ?? 0) + 1)
    }
    const spread = spreadBurstRatings(current, chosen)
    current.history.push({ groupIndex: 0, chosen: [...chosen, ...spread] })
    current.groupIndex += 1

    undoLastStep(current)

    expect(current.ratings.a).toBe(2)
    expect(current.ratings.a2).toBe(2)
    // 仲間は survivors に入れない。入れると畳んだ意味が消える。
    expect(current.survivors).toEqual([])
  })
})

// ---------------------------------------------------------------------------
// Routine 7: まとめの中身を編集する
// ---------------------------------------------------------------------------

describe('spreadBurstRatings は決着済みを上書きしない', () => {
  const session = () => {
    const built = makeTestSession([['a', 'b']])
    built.burstMembers = { a: ['a', 'a2', 'a3'] }
    built.ratings = { a: 2, a2: 2, a3: 2, b: 2 }
    built.targetRating = 2
    return built
  }

  it('まとめの中で下げた星は、代表が通っても戻らない', () => {
    const current = session()
    current.ratings.a2 = 1          // 明らかに脱落として −1
    current.burstSettled = ['a2']
    current.ratings.a = 3
    expect(spreadBurstRatings(current, ['a'])).toEqual(['a3'])
    expect(current.ratings.a2).toBe(1)
    expect(current.ratings.a3).toBe(3)
  })

  it('まとめの中で★5に確定した写真も代表の星に引き下げられない', () => {
    const current = session()
    current.ratings.a2 = MAX_RATING
    current.burstSettled = ['a2']
    current.ratings.a = 3
    spreadBurstRatings(current, ['a'])
    expect(current.ratings.a2).toBe(MAX_RATING)
  })

  it('決着済みが空なら従来どおり全員に配る', () => {
    const current = session()
    current.burstSettled = []
    current.ratings.a = 3
    expect(spreadBurstRatings(current, ['a'])).toEqual(['a2', 'a3'])
  })
})

describe('insertIntoUpcoming', () => {
  const session = () => {
    const built = makeTestSession([['a', 'b'], ['c', 'd'], ['e', 'f']])
    built.settings = { groupSize: 2, groupBursts: true }
    built.groupIndex = 1 // いま c,d を見ている
    return built
  }

  it('いま見ているグループは絶対に変えない', () => {
    const current = session()
    insertIntoUpcoming(current, ['x', 'y'])
    expect(current.groups[1]).toEqual(['c', 'd'])
  })

  it('済んだグループも変えない', () => {
    const current = session()
    insertIntoUpcoming(current, ['x', 'y'])
    expect(current.groups[0]).toEqual(['a', 'b'])
  })

  it('外した写真は次に見る分の先頭に入る', () => {
    const current = session()
    insertIntoUpcoming(current, ['x', 'y'])
    expect(current.groups.slice(2)).toEqual([['x', 'y'], ['e', 'f']])
  })

  it('候補にも戻す。戻さないと次のラウンドから消える', () => {
    const current = session()
    insertIntoUpcoming(current, ['x'])
    expect(current.candidates).toContain('x')
  })

  it('端数が1枚になったら比較にならないのでそのまま通す', () => {
    const current = session()
    insertIntoUpcoming(current, ['x'])
    // 未処理は x,e,f の3枚。2枚ずつだと1枚余る。
    expect(current.groups.slice(2)).toEqual([['x', 'e']])
    expect(current.survivors).toEqual(['f'])
  })

  it('既にグループに居る写真は二重に入れない', () => {
    const current = session()
    insertIntoUpcoming(current, ['e'])
    expect(current.groups.slice(2)).toEqual([['e', 'f']])
    expect(current.groups.flat().filter(id => id === 'e')).toHaveLength(1)
  })

  it('最後のグループを見ているときは、そのうしろに足す', () => {
    const current = session()
    current.groupIndex = 2
    insertIntoUpcoming(current, ['x', 'y'])
    expect(current.groups).toEqual([['a', 'b'], ['c', 'd'], ['e', 'f'], ['x', 'y']])
  })

  it('戻る履歴の位置は動かさない', () => {
    const current = session()
    insertIntoUpcoming(current, ['x', 'y'])
    expect(current.groupIndex).toBe(1)
  })
})
