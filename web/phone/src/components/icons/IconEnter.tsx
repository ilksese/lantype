import { Stroke, type IconProps } from './IconBase'

export function IconEnter(props: IconProps) {
  return (
    <Stroke {...props}>
      <polyline points="9 10 4 15 9 20" />
      <path d="M20 4v7a4 4 0 0 1-4 4H4" />
    </Stroke>
  )
}
