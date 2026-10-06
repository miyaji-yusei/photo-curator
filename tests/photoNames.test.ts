// R1: どの名前を写真の候補にするか・組の RAW をどう除くかの共通の表を、Web の走査（scanFolder）に通す。
// 同じ `core/tests/fixtures/photo-names.json` を PC（src-tauri/src/tests.rs）と
// Android（PhotoNamesFixtureTest）も読む。食い違いは表の `known_differences.web`。
import { describe, expect, it } from 'vitest'
import type { SourceEntry, SourceIO } from '~/composables/backends/web/sourceIO'
import { scanFolder } from '~/utils/folderScan'
import fixture from '~/core/tests/fixtures/photo-names.json'

interface Difference { reason: string, pair_on?: string[], pair_off?: string[] }
interface Case {
  id: string
  files: string[]
  pair_on: string[]
  pair_off: string[]
  known_differences?: Record<string, Difference>
}
const cases = fixture.cases as Case[]

const noSidecar = {
  sidecarAccess: () => Promise.resolve('none' as const),
  readSidecar: () => Promise.resolve(null),
  writeSidecar: () => Promise.reject(new Error('not used')),
  writeSidecarChecked: () => Promise.reject(new Error('not used')),
  asideSidecar: () => Promise.reject(new Error('not used'))
}

/** 相対パスの一覧から、フォルダの木を持つ偽の出所を作る。 */
function folderOf(files: string[]): SourceIO {
  const tree = new Map<string, Map<string, SourceEntry>>()
  const ensure = (sub: string): Map<string, SourceEntry> => {
    let entries = tree.get(sub)
    if (!entries) {
      entries = new Map()
      tree.set(sub, entries)
      if (sub !== '') {
        const cut = sub.lastIndexOf('/')
        const parent = cut < 0 ? '' : sub.slice(0, cut)
        ensure(parent).set(sub.slice(cut + 1), { name: sub.slice(cut + 1), isDirectory: true, size: 0, mtimeMs: 1 })
      }
    }
    return entries
  }
  for (const file of files) {
    const cut = file.lastIndexOf('/')
    const sub = cut < 0 ? '' : file.slice(0, cut)
    const name = file.slice(cut + 1)
    ensure(sub).set(name, { name, isDirectory: false, size: 10, mtimeMs: 1 })
  }
  return {
    list: (sub: string) => Promise.resolve([...(tree.get(sub)?.values() ?? [])]),
    readFile: () => Promise.reject(new Error('not used')),
    ...noSidecar
  }
}

function expected(item: Case, key: 'pair_on' | 'pair_off'): string[] {
  return [...(item.known_differences?.web?.[key] ?? item[key])].sort()
}

describe('photo-names.json（Web の走査）', () => {
  it('表に 10 件以上のケースがある', () => {
    expect(cases.length).toBeGreaterThanOrEqual(10)
  })

  for (const item of cases) {
    for (const [pairRawJpeg, key] of [[true, 'pair_on'], [false, 'pair_off']] as const) {
      it(`${item.id}（${key}）`, async () => {
        const found = await scanFolder(folderOf(item.files), { pairRawJpeg })
        expect(found.map(file => file.relativePath).sort()).toEqual(expected(item, key))
      })
    }
  }

  it('known_differences は表の既定と本当に違う', () => {
    for (const item of cases) {
      for (const [implementation, difference] of Object.entries(item.known_differences ?? {})) {
        expect(['pc', 'web', 'android']).toContain(implementation)
        expect(difference.reason).toBeTruthy()
        for (const key of ['pair_on', 'pair_off'] as const) {
          if (difference[key]) {
            expect([...difference[key]].sort(), `${item.id} ${implementation} ${key}`).not.toEqual([...item[key]].sort())
          }
        }
      }
    }
  })
})
