// 渠道 models 字段（逗号分隔）解析与聚合。

/** "a, b,,c " -> ["a","b","c"] */
export function splitCsv(v?: string | null): string[] {
  return (v || '')
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean)
}

/** 从渠道列表聚合去重排序后的模型名（令牌限模型选择器 / 游乐场模型下拉共用）。 */
export function uniqueModelsFromChannels(channels: Array<{ models?: string }>): string[] {
  const set = new Set<string>()
  for (const ch of channels) {
    for (const m of splitCsv(ch.models)) set.add(m)
  }
  return Array.from(set).sort()
}
