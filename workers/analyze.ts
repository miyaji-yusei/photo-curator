/// <reference lib="webworker" />
/**
 * 解析を裏で回すワーカー。復号と縮小は重いので、選別の操作を止めないよう
 * メインスレッドから追い出す。中身は `utils/analyzePhoto.ts` と共有していて、
 * ワーカーが使えない環境ではメインスレッドで同じ関数が動く。
 *
 * `SharedArrayBuffer` は使わない。GitHub Pages は HTTP ヘッダを設定できず
 * COOP/COEP を張れないため、そもそも使えない。
 */
import type { AnalyzedPhoto } from '~/utils/analyzePhoto'
import { analyzePhotoFile } from '~/utils/analyzePhoto'

export interface AnalyzeWorkerRequest {
  id: string
  file: File
}

export interface AnalyzeWorkerResponse extends AnalyzedPhoto {
  id: string
}

self.onmessage = async (event: MessageEvent<AnalyzeWorkerRequest>) => {
  const { id, file } = event.data
  const analyzed = await analyzePhotoFile(file)
  const response: AnalyzeWorkerResponse = { id, ...analyzed }
  // Blob は構造化複製で渡る（転送不可なのでそのまま postMessage する）。
  self.postMessage(response)
}
