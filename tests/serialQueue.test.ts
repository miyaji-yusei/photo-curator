import { describe, expect, it } from 'vitest'
import { createSerialQueue } from '~/utils/serialQueue'

const tick = () => new Promise(resolve => setTimeout(resolve, 0))

function gated() {
  const log: string[] = []
  const gates: Array<() => void> = []
  const task = (name: string) => () => new Promise<void>((resolve) => {
    log.push(`start ${name}`)
    gates.push(() => { log.push(`end ${name}`); resolve() })
  })
  return { log, task, release: () => gates.shift()?.() }
}

describe('serialQueue', () => {
  it('enqueue は待たず、入れた順に 1 本ずつ走る（前が終わるまで次は始まらない）', async () => {
    const g = gated()
    const queue = createSerialQueue(() => {})
    queue.enqueue(g.task('a'))
    queue.enqueue(g.task('b'))
    queue.enqueue(g.task('c'))
    await tick()
    expect(g.log).toEqual(['start a'])
    g.release(); await tick()
    expect(g.log).toEqual(['start a', 'end a', 'start b'])
    g.release(); await tick(); g.release()
    await queue.flush()
    expect(g.log).toEqual(['start a', 'end a', 'start b', 'end b', 'start c', 'end c'])
    expect(queue.busy).toBe(false)
  })

  it('flush は、待っている間に足されたものも含めて全部終わるまで待つ', async () => {
    const g = gated()
    const queue = createSerialQueue(() => {})
    queue.enqueue(g.task('a'))
    let flushed = false
    const done = queue.flush().then(() => { flushed = true })
    await tick()
    queue.enqueue(g.task('b'))
    g.release(); await tick()
    expect(flushed).toBe(false)
    g.release(); await done
    expect(flushed).toBe(true)
    expect(g.log.at(-1)).toBe('end b')
  })

  it('失敗は onError へ渡して、続きを走らせる', async () => {
    const errors: unknown[] = []
    const ran: string[] = []
    const queue = createSerialQueue(cause => errors.push(cause))
    queue.enqueue(async () => { throw new Error('boom') })
    queue.enqueue(async () => { ran.push('next') })
    await queue.flush()
    expect(errors).toHaveLength(1)
    expect(ran).toEqual(['next'])
  })

  it('afterDrain は空になったとき 1 回だけ（連続して入れたぶんは畳む）。flush はその終わりまで待つ', async () => {
    const log: string[] = []
    const queue = createSerialQueue(() => {}, async () => { await tick(); log.push('drained') })
    queue.enqueue(async () => { log.push('a') })
    queue.enqueue(async () => { log.push('b') })
    queue.enqueue(async () => { log.push('c') })
    await queue.flush()
    expect(log).toEqual(['a', 'b', 'c', 'drained'])
  })

  it('何も無ければ flush はすぐ返る', async () => {
    await createSerialQueue(() => {}).flush()
  })
})
