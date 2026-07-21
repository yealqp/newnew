// 跨页面共享的展示格式化工具。

/** Token 数量缩写：>=1M -> "1.23M"，>=1K -> "1.2K"，0 使用 zeroAs（渠道页传 "-"）。 */
export function formatTokenCount(n: number | undefined | null, zeroAs = '0'): string {
  const v = n || 0
  if (v === 0) return zeroAs
  if (v >= 1_000_000) return `${(v / 1_000_000).toFixed(2)}M`
  if (v >= 1_000) return `${(v / 1_000).toFixed(1)}K`
  return String(v)
}

/** 人民币费用格式化；精度由调用方指定（仪表盘 4 位、日志 6 位）。 */
export function formatCostRMB(n: number | undefined | null, decimals = 6): string {
  return `¥${(n || 0).toFixed(decimals)}`
}

/** tokens/s 速度；耗时或 token 数不合法时返回 null（调用方决定占位符）。 */
export function formatTokenSpeed(totalTokens: number, durationMs: number): string | null {
  if (!(durationMs > 0) || !(totalTokens > 0)) return null
  return `${Math.round(totalTokens / (durationMs / 1000)).toLocaleString()} t/s`
}

/** antd Select 通用的大小写不敏感搜索。 */
export function filterOptionBySearch(input: string, option?: { value?: unknown }): boolean {
  return String(option?.value ?? '').toLowerCase().includes(input.toLowerCase())
}
