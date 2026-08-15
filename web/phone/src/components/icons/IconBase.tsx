import type { ComponentChildren } from 'preact'

export interface IconProps {
  size?: number
  color?: string
  className?: string
}

export function Stroke({ size = 24, color, className, children }: IconProps & { children: ComponentChildren }) {
  return (
    <svg
      className={className}
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke={color ?? 'currentColor'}
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      xmlns="http://www.w3.org/2000/svg"
      aria-hidden="true"
    >
      {children}
    </svg>
  )
}

export function Solid({ size = 24, color, className, children }: IconProps & { children: ComponentChildren }) {
  return (
    <svg
      className={className}
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill={color ?? 'currentColor'}
      xmlns="http://www.w3.org/2000/svg"
      aria-hidden="true"
    >
      {children}
    </svg>
  )
}
