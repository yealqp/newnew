import { Button, Drawer } from 'antd'
import type { CSSProperties, ReactNode } from 'react'

/** Drawer 宽度自适应小屏。 */
export function responsiveWidth(preferred: number): number {
  return Math.min(preferred, typeof window !== 'undefined' ? window.innerWidth : preferred)
}

/**
 * 编辑/新建表单抽屉的共用外壳：右侧、destroyOnHidden、
 * 底部「取消 / 保存」页脚（渠道页与令牌页共用）。
 */
export default function FormDrawer({
  title,
  open,
  onClose,
  onSave,
  saving,
  width = 480,
  bodyStyle,
  children,
}: {
  title: ReactNode
  open: boolean
  onClose: () => void
  onSave: () => void
  saving?: boolean
  width?: number
  bodyStyle?: CSSProperties
  children: ReactNode
}) {
  return (
    <Drawer
      title={title}
      open={open}
      onClose={onClose}
      width={responsiveWidth(width)}
      destroyOnHidden
      placement="right"
      styles={bodyStyle ? { body: bodyStyle } : undefined}
      footer={
        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
          <Button onClick={onClose}>取消</Button>
          <Button type="primary" loading={saving} onClick={onSave}>
            保存
          </Button>
        </div>
      }
    >
      {children}
    </Drawer>
  )
}
