/**
 * 解析の失敗の扱い（U58。Web）。**やり直すかどうかの判断だけ**で、読み書きはしない。
 *
 * - 一時的（読めない・メモリ不足・ワーカーの異常）: 次に開いたとき・再読み込みでもう一度試す
 * - 非対応（読めたのに復号できない）: 原本（大きさ・更新時刻）が変わるまで試さない
 * - ハッシュ値だけ作れない（小さすぎる）: 絵はあるので選別できる。試さず、数えず、一覧にも出さない
 */
import type { StoredErrorKind, StoredPhoto } from '~/utils/browserStore'

type Row = Pick<StoredPhoto, 'isMissing' | 'dHash' | 'analysisError' | 'analysisErrorKind'>

/** 行の失敗の種類。失敗が無ければ null。種類の無い失敗（U58 より前）は一時的。 */
export function errorKindOf(row: Pick<StoredPhoto, 'analysisError' | 'analysisErrorKind'>): StoredErrorKind | null {
  return row.analysisError === null ? null : (row.analysisErrorKind ?? 'transient')
}

/** 解析がまだ要る行か（準備の「未処理」に数える・次の解析の対象にする）。非対応は含まない。 */
export function needsAnalysis(row: Row): boolean {
  if (row.isMissing || row.dHash !== null) return false
  const kind = errorKindOf(row)
  return kind === null || kind === 'transient'
}

/** 非対応の行で、原本が前に失敗したときのまま変わっていないか。 */
export function isSettledUnsupported(
  row: Pick<StoredPhoto, 'analysisError' | 'analysisErrorKind' | 'analysisFailedSize' | 'analysisFailedModified'>,
  file: { size: number, lastModified: number }
): boolean {
  return errorKindOf(row) === 'unsupported'
    && row.analysisFailedSize === file.size
    && row.analysisFailedModified === file.lastModified
}

/** 非対応で解析の対象外（原本が変わったかは、解析の側で調べる）。 */
export const isUnsupportedRow = (row: Row): boolean => !row.isMissing && errorKindOf(row) === 'unsupported'

/** 選別の対象になる行（非対応を除く）。ホームの「◯枚」。 */
export const selectableCount = (rows: readonly Row[]): number =>
  rows.filter(row => !row.isMissing && errorKindOf(row) !== 'unsupported').length
