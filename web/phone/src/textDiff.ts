export interface TextDiff {
  backspace: number
  text: string
}

interface SegmentPart {
  segment: string
}

interface SegmenterLike {
  segment: (value: string) => Iterable<SegmentPart>
}

interface SegmenterConstructor {
  new (locale?: string, options?: { granularity: 'grapheme' }): SegmenterLike
}

const MARK = /\p{Mark}/u
const ZERO_WIDTH_JOINER = '\u200d'

let segmenter: SegmenterLike | null | undefined

function getSegmenter(): SegmenterLike | null {
  if (segmenter !== undefined) return segmenter
  if (typeof Intl === 'undefined') return (segmenter = null)
  const Constructor = (Intl as typeof Intl & { Segmenter?: SegmenterConstructor }).Segmenter
  segmenter = Constructor ? new Constructor(undefined, { granularity: 'grapheme' }) : null
  return segmenter
}

export function supportsGraphemeDiff(): boolean {
  return getSegmenter() !== null
}

function isRegionalIndicator(value: string): boolean {
  const point = value.codePointAt(0) ?? 0
  return point >= 0x1f1e6 && point <= 0x1f1ff
}

function isExtension(value: string): boolean {
  const point = value.codePointAt(0) ?? 0
  return MARK.test(value)
    || (point >= 0xfe00 && point <= 0xfe0f)
    || (point >= 0xe0100 && point <= 0xe01ef)
    || (point >= 0x1f3fb && point <= 0x1f3ff)
    || (point >= 0xe0020 && point <= 0xe007f)
}

function splitFallback(value: string): string[] {
  const clusters: string[] = []
  let previous = ''
  let joinNext = false
  let regionalRun = 0

  for (const point of Array.from(value)) {
    const regional = isRegionalIndicator(point)
    const append = clusters.length > 0 && (
      joinNext
      || point === ZERO_WIDTH_JOINER
      || isExtension(point)
      || (previous === '\r' && point === '\n')
      || (regional && regionalRun % 2 === 1)
    )

    if (append) clusters[clusters.length - 1] += point
    else clusters.push(point)

    if (regional) regionalRun += 1
    else if (!isExtension(point) && point !== ZERO_WIDTH_JOINER) regionalRun = 0
    joinNext = point === ZERO_WIDTH_JOINER
    previous = point
  }

  return clusters
}

export function splitGraphemes(value: string): string[] {
  const currentSegmenter = getSegmenter()
  if (!currentSegmenter) return splitFallback(value)
  return Array.from(currentSegmenter.segment(value), (part) => part.segment)
}

export function getTextDiff(previous: string, next: string): TextDiff {
  const previousClusters = splitGraphemes(previous)
  const nextClusters = splitGraphemes(next)
  let commonPrefix = 0

  while (
    commonPrefix < previousClusters.length
    && commonPrefix < nextClusters.length
    && previousClusters[commonPrefix] === nextClusters[commonPrefix]
  ) {
    commonPrefix += 1
  }

  return {
    backspace: previousClusters.length - commonPrefix,
    text: nextClusters.slice(commonPrefix).join(''),
  }
}
