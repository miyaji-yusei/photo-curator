/**
 * 画面の幅で分岐するための土台。**環境名では分岐しない**（CON-5）。
 *
 * 検収は 3 サイズ（設計 06 章 E-5）:
 * - 933×704（Fold 開・横長）→ medium・landscape
 * - 476×752（Fold カバー・縦長）→ narrow・portrait
 * - 1440×900（PC）→ wide・landscape
 */
import { computed, onMounted, onUnmounted, ref } from 'vue'

export type WidthClass = 'narrow' | 'medium' | 'wide'

const NARROW_MAX = 599
const WIDE_MIN = 1100

export function widthClassOf(width: number): WidthClass {
  if (width <= NARROW_MAX) return 'narrow'
  if (width >= WIDE_MIN) return 'wide'
  return 'medium'
}

export function useLayout() {
  const width = ref(typeof window === 'undefined' ? 1440 : window.innerWidth)
  const height = ref(typeof window === 'undefined' ? 900 : window.innerHeight)

  const onResize = () => {
    width.value = window.innerWidth
    height.value = window.innerHeight
  }

  onMounted(() => {
    onResize()
    window.addEventListener('resize', onResize)
  })
  onUnmounted(() => window.removeEventListener('resize', onResize))

  const widthClass = computed(() => widthClassOf(width.value))
  const isNarrow = computed(() => widthClass.value === 'narrow')
  const isWide = computed(() => widthClass.value === 'wide')
  const isLandscape = computed(() => width.value > height.value)

  return { width, height, widthClass, isNarrow, isWide, isLandscape }
}
