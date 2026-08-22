import { Stroke, type IconProps } from './IconBase'

export function IconPhrase(props: IconProps) {
  return (
    <Stroke {...props}>
      <path d="M4 5h16" />
      <path d="M4 12h11" />
      <path d="M4 19h7" />
      <path d="M18 14v6" />
      <path d="M15 17h6" />
    </Stroke>
  )
}
