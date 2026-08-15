import { Stroke, type IconProps } from './IconBase'

export function IconClear(props: IconProps) {
  return (
    <Stroke {...props}>
      <line x1="18" y1="6" x2="6" y2="18" />
      <line x1="6" y1="6" x2="18" y2="18" />
    </Stroke>
  )
}
