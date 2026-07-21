// OpenGate 品牌 Logo：与桌面端 icon.svg 同款（左右空心半方 + 中央四角星）。
// 整体顺时针旋转 90° 并放大；沿用 DESIGN.md 的珊瑚色 token。默认 32 尺寸可缩放。
export function GateLogo({ size = 32 }: { size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 256 256"
      fill="none"
      aria-hidden="true"
      style={{ display: 'block', flexShrink: 0 }}
    >
      <g transform="translate(128 128) rotate(90) scale(1.2) translate(-128 -128)">
        <path
          d="M64 96c0-26.51 21.49-48 48-48h32c26.51 0 48 21.49 48 48"
          stroke="#cc785c"
          strokeWidth="18"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
        <path
          d="M192 160c0 26.51-21.49 48-48 48h-32c-26.51 0-48-21.49-48-48"
          stroke="#a9583e"
          strokeWidth="18"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
        <path
          d="M128 96 L132 124 L160 128 L132 132 L128 160 L124 132 L96 128 L124 124 Z"
          fill="#cc785c"
        />
        <circle cx="128" cy="128" r="5" fill="#a9583e" />
      </g>
    </svg>
  )
}