/**
 * `createCurator` を画面なしで動かすための道具。
 *
 * - Nuxt の自動 import（`ref`・`computed`・`useNotice` など）を、vitest ではグローバルに差す
 * - 偽の `desktop`（`PhotoBackend`）を作る。**書いていないメソッドは「呼ばれたら undefined を返す」**
 *   だけにして、テストが使うものだけ本物らしい値を返す
 */
import * as vue from 'vue'
import { vi } from 'vitest'
import * as core from '~/lib/core'
import type { PhotoBackend } from '~/composables/photoBackend'
import type { Photo, Project } from '~/types/photo'
import { capabilitiesFor } from '~/utils/capabilities'
import { wasmBytes } from './wasmBytes.mjs'

let loaded: Promise<typeof import('~/composables/useCurator')> | null = null

/** グローバルを差してから `useCurator` を読み込む（1 回だけ）。core（wasm）も初期化する。 */
export function loadCuratorModule() {
  if (!loaded) {
    core.initWithBytes(wasmBytes())
    for (const [name, value] of Object.entries(vue)) vi.stubGlobal(name, value)
    loaded = (async () => {
      const notice = await import('~/composables/useNotice')
      vi.stubGlobal('useNotice', notice.useNotice)
      vi.stubGlobal('useDesktop', () => { throw new Error('テストでは desktop を渡す') })
      return import('~/composables/useCurator')
    })()
  }
  return loaded
}

export const project = (id: string, overrides: Partial<Project> = {}): Project => ({
  id, name: id, folderPath: `/photos/${id}`, photoCount: 10, status: 'ready',
  createdAt: 1, updatedAt: 1, burstThreshold: null, burstThresholdLearnedAt: null,
  sourceKind: 'folder', ...overrides
})

/** 撮影順に 1 分おきの写真 `count` 枚。`relativePath` は `IMG_<n>.JPG`、id は `<projectId>-<n>`。 */
export function photos(projectId: string, count: number): Photo[] {
  return Array.from({ length: count }, (_, index) => ({
    id: `${projectId}-${index}`, projectId, path: `blob:${projectId}-${index}`,
    relativePath: `IMG_${index}.JPG`, name: `IMG_${index}.JPG`, capturedAt: 1_700_000_000_000 + index * 60_000,
    dHash: null, rating: 0, thumbnailPath: `blob:t-${projectId}-${index}`, displayPath: `blob:d-${projectId}-${index}`
  }))
}

/** 外から終わらせられる Promise。 */
export function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (cause: unknown) => void
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail })
  return { promise, resolve, reject }
}

export const settle = () => new Promise(resolve => setTimeout(resolve, 0))

/**
 * 偽の `desktop`。`overrides` に書いたものだけが本物らしく動く。
 * 呼び出しは `calls` に残る（メソッド名 → 引数の並び）。
 */
export function fakeDesktop(overrides: Partial<Record<keyof PhotoBackend, unknown>> = {}) {
  const calls: Record<string, unknown[][]> = {}
  const base: Record<string, unknown> = {
    kind: 'local',
    capabilities: capabilitiesFor('browser', { directoryPicker: false }),
    listProjects: async () => [],
    getAnalysisBacklog: async () => 0,
    getDisplayBacklog: async () => 0,
    getProjectPhotoPage: async () => ({ photos: [], total: 0 }),
    getSelectionSummary: async () => ({ counts: [0, 0, 0, 0, 0, 0], total: 0 }),
    getDisplaySettings: async () => ({
      edge: 1024, choices: [1024, 1536], defaultEdge: 1024, largeEdge: 1536, canRebuild: false
    }),
    loadSession: async () => null,
    saveSession: async () => undefined,
    saveSelectionResults: async () => undefined,
    getPairOverrides: async () => [],
    sidecarSupported: async () => 'none',
    loadSidecarState: async () => ({ seenAt: 0, seenBy: '', localChanged: false }),
    onProjectProgress: async () => () => undefined,
    photoDisplayUrl: (photo: Photo) => photo.displayPath ?? photo.path,
    photoThumbnailUrl: (photo: Photo) => photo.thumbnailPath ?? photo.path,
    photoUrl: (path: string) => path,
    startBackgroundAnalysis: async () => undefined,
    startDisplayGeneration: async () => undefined,
    ...overrides
  }
  const desktop = new Proxy(base, {
    get(target, key: string) {
      if (key === 'then') return undefined
      const value = key in target ? target[key] : async () => undefined
      if (typeof value !== 'function') return value
      return (...args: unknown[]) => {
        (calls[key] ??= []).push(args)
        return (value as (...a: unknown[]) => unknown)(...args)
      }
    }
  }) as unknown as PhotoBackend
  return { desktop, calls }
}
