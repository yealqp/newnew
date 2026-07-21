import { useState } from 'react'

/**
 * 键值行编辑器的通用状态（渠道页的定价表 / 模型映射表共用）：
 * 增行、按索引改字段、删行。
 */
export function useEditableRows<T>(makeEmpty: () => T) {
  const [rows, setRows] = useState<T[]>([])

  const add = () => setRows((prev) => [...prev, makeEmpty()])

  const update = <K extends keyof T>(index: number, key: K, value: T[K]) =>
    setRows((prev) => prev.map((r, i) => (i === index ? { ...r, [key]: value } : r)))

  const remove = (index: number) => setRows((prev) => prev.filter((_, i) => i !== index))

  return { rows, setRows, add, update, remove }
}
