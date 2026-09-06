import type { Photo, PreviewState } from '~/types/photo'

/**
 * 1 枚ぶんの準備状態。**空のタイルに理由を出すために要る。**
 *
 * 以前は何も無いタイルが並ぶだけで、まだ作っていないのか、読めなかったのか、
 * 対応していない形式なのかが分からなかった。
 *
 * `analysing` は「いま解析が走っているか」。走っていれば作成中、
 * 止まっていれば順番待ち、と読み分ける。
 */
export function previewStateOf(photo: Photo, analysing: boolean): PreviewState {
  if (photo.thumbnailPath) return 'ready'
  const reason = photo.analysisError ?? ''
  // 非対応の形式は「壊れている」とは違う。直しようがないので言い方を分ける。
  if (reason.includes('非対応')) return 'unsupported'
  if (reason) return 'failed'
  return analysing ? 'generating' : 'queued'
}

/** 状態ごとの見え方。文言もここに置き、画面には出し分けだけをさせる。 */
export const PREVIEW_STATE_TEXT: Record<PreviewState, { icon: string, label: string, color?: string }> = {
  ready: { icon: '', label: '' },
  generating: { icon: 'mdi-sync', label: '作成中', color: 'secondary' },
  queued: { icon: 'mdi-clock-outline', label: '待機' },
  failed: { icon: 'mdi-image-broken-variant', label: '読めません', color: 'error' },
  unsupported: { icon: 'mdi-cancel', label: '非対応の形式' }
}
