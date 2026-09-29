import { describe, expect, it } from 'vitest'
import { createSaveQueue } from '~/utils/saveQueue'

/** 呼ぶたびに、外から終わらせられる書き込みを作る。 */
function gatedWriter() {
  const log: string[] = []
  const gates: Array<() => void> = []
  const write = (projectId: string, envelope: string | null) => new Promise<void>((resolve) => {
    log.push(`${projectId}:${envelope}`)
    gates.push(resolve)
  })
  return { write, log, release: () => gates.shift()?.() }
}

const tick = () => new Promise(resolve => setTimeout(resolve, 0))

describe('saveQueue', () => {
  it('enqueue は待たずに戻り、書き込みは 1 本ずつ', async () => {
    const w = gatedWriter()
    const queue = createSaveQueue<string>(w.write, () => {})
    queue.enqueue('p', 'a')
    queue.enqueue('q', 'b')
    await tick()
    expect(w.log).toEqual(['p:a'])
    w.release(); await tick()
    expect(w.log).toEqual(['p:a', 'q:b'])
    w.release(); await queue.flush()
    expect(queue.busy).toBe(false)
  })

  it('未書き込みは最新の 1 つだけ残す（書き込み中のものは邪魔しない）', async () => {
    const w = gatedWriter()
    const queue = createSaveQueue<string>(w.write, () => {})
    queue.enqueue('p', '1')
    await tick()
    queue.enqueue('p', '2')
    queue.enqueue('p', '3')
    w.release(); await tick()
    w.release(); await queue.flush()
    expect(w.log).toEqual(['p:1', 'p:3'])
  })

  it('null（途中の選別を消す）も最新として残り、順序が入れ替わらない', async () => {
    const w = gatedWriter()
    const queue = createSaveQueue<string>(w.write, () => {})
    queue.enqueue('p', 'a')
    await tick()
    queue.enqueue('p', 'b')
    queue.enqueue('p', null)
    w.release(); await tick(); w.release()
    await queue.flush()
    expect(w.log).toEqual(['p:a', 'p:null'])
  })

  it('flush は書き終わるまで待つ', async () => {
    const w = gatedWriter()
    const queue = createSaveQueue<string>(w.write, () => {})
    queue.enqueue('p', 'a')
    let flushed = false
    const done = queue.flush().then(() => { flushed = true })
    await tick()
    expect(flushed).toBe(false)
    w.release(); await done
    expect(flushed).toBe(true)
  })

  it('flush は書き込み中に足されたものも待つ', async () => {
    const w = gatedWriter()
    const queue = createSaveQueue<string>(w.write, () => {})
    queue.enqueue('p', 'a')
    await tick()
    const done = queue.flush()
    queue.enqueue('q', 'b')
    w.release(); await tick(); w.release()
    await done
    expect(w.log).toEqual(['p:a', 'q:b'])
    expect(queue.busy).toBe(false)
  })

  it('失敗は onError へ。その 1 件を捨てて続きを書く', async () => {
    const errors: unknown[] = []
    const written: string[] = []
    const queue = createSaveQueue<string>(async (projectId, envelope) => {
      if (envelope === 'bad') throw new Error('disk full')
      written.push(`${projectId}:${envelope}`)
    }, cause => errors.push(cause))
    queue.enqueue('p', 'bad')
    queue.enqueue('q', 'ok')
    await queue.flush()
    expect(errors).toHaveLength(1)
    expect((errors[0] as Error).message).toBe('disk full')
    expect(written).toEqual(['q:ok'])
    queue.enqueue('p', 'again'); await queue.flush()
    expect(written).toEqual(['q:ok', 'p:again'])
  })

  it('何も無ければ flush はすぐ返る', async () => {
    const queue = createSaveQueue<string>(async () => {}, () => {})
    await queue.flush()
    expect(queue.busy).toBe(false)
  })
})
