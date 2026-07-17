import { useEffect, useMemo, useRef, useState } from 'react'
import { Input, Popover } from 'antd'
import { ICON_LIST } from '../utils/lobeIcons'
import type { IconEntry } from '../utils/lobeIcons'

interface IconPickerProps {
  value?: string
  onChange?: (id: string) => void
}

function Chunk({
  entry,
  active,
  onSelect,
}: {
  entry: IconEntry
  active: boolean
  onSelect: (id: string) => void
}) {
  const Comp = entry.component
  const ColorComp = (Comp as any)?.Color
  const Target = ColorComp || Comp
  return (
    <button
      type="button"
      onClick={() => onSelect(entry.id)}
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 6,
        padding: '6px 10px',
        border: active ? '1px solid var(--primary)' : '1px solid transparent',
        borderRadius: 6,
        background: active ? 'var(--primary-soft)' : 'transparent',
        cursor: 'pointer',
        color: 'var(--foreground)',
        fontSize: 12,
        fontFamily: 'inherit',
        transition: 'background 0.1s, border-color 0.15s',
      }}
      title={entry.id}
      onPointerEnter={(e) => {
        if (!active) {
          e.currentTarget.style.background = 'var(--muted)'
        }
      }}
      onPointerLeave={(e) => {
        if (!active) {
          e.currentTarget.style.background = 'transparent'
        }
      }}
    >
      <Target size={18} />
      <span style={{ whiteSpace: 'nowrap' }}>{entry.id}</span>
    </button>
  )
}

export default function IconPicker({ value, onChange }: IconPickerProps) {
  const [open, setOpen] = useState(false)
  const [search, setSearch] = useState('')
  const inputRef = useRef<any>(null)

  // Debounced search
  const [debounced, setDebounced] = useState('')
  useEffect(() => {
    const t = setTimeout(() => setDebounced(search), 250)
    return () => clearTimeout(t)
  }, [search])

  const filtered = useMemo(() => {
    const q = debounced.trim().toLowerCase()
    if (!q) return ICON_LIST
    return ICON_LIST.filter(
      (e) =>
        e.id.toLowerCase().includes(q) ||
        e.title.toLowerCase().includes(q),
    )
  }, [debounced])

  const handleOpenChange = (v: boolean) => {
    setOpen(v)
    if (v) {
      setSearch('')
      setDebounced('')
      // focus input on next tick after popover mounts
      setTimeout(() => inputRef.current?.focus(), 50)
    }
  }

  const handleSelect = (id: string) => {
    onChange?.(id)
    setOpen(false)
  }

  const CurrentIcon = value ? ICON_LIST.find((e) => e.id === value) : null

  return (
    <Popover
      open={open}
      onOpenChange={handleOpenChange}
      trigger="click"
      placement="bottomLeft"
      arrow={false}
      styles={{
        content: {
          padding: 0,
          background: 'var(--popover)',
          border: '1px solid var(--border)',
          borderRadius: 10,
          maxHeight: 420,
          overflow: 'hidden',
          display: 'flex',
          flexDirection: 'column',
        },
      }}
      content={
        <div style={{ width: 340 }}>
          {/* Search */}
          <div style={{ padding: '8px 10px', borderBottom: '1px solid var(--border)' }}>
            <Input
              ref={inputRef}
              allowClear
              placeholder="搜索图标…"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && filtered.length === 1) {
                  handleSelect(filtered[0].id)
                }
              }}
              style={{ fontSize: 13 }}
            />
          </div>

          {/* Grid */}
          <div
            style={{
              padding: '6px 8px',
              overflow: 'auto',
              maxHeight: 320,
              display: 'flex',
              flexWrap: 'wrap',
              gap: 2,
            }}
          >
            {filtered.length === 0 ? (
              <div style={{ color: 'var(--text-dim)', padding: 16, textAlign: 'center', width: '100%', fontSize: 13 }}>
                未找到 “{debounced}”
              </div>
            ) : (
              filtered.map((entry) => (
                <Chunk
                  key={entry.id}
                  entry={entry}
                  active={entry.id === value}
                  onSelect={handleSelect}
                />
              ))
            )}
          </div>
        </div>
      }
    >
      <button
        type="button"
        style={{
          display: 'inline-flex',
          alignItems: 'center',
          gap: 6,
          padding: '4px 10px',
          border: '1px solid var(--border)',
          borderRadius: 6,
          background: 'var(--muted)',
          cursor: 'pointer',
          color: 'var(--foreground)',
          fontSize: 13,
          fontFamily: 'inherit',
          whiteSpace: 'nowrap',
          minWidth: 60,
          justifyContent: 'center',
          transition: 'border-color 0.15s',
        }}
        onPointerEnter={(e) => {
          e.currentTarget.style.borderColor = 'var(--primary)'
        }}
        onPointerLeave={(e) => {
          e.currentTarget.style.borderColor = 'var(--border)'
        }}
      >
        {CurrentIcon ? (
          <>
            {(() => {
              const Comp = CurrentIcon.component
              const C = (Comp as any)?.Color || Comp
              return <C size={16} />
            })()}
            {CurrentIcon.id}
          </>
        ) : (
          <>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <circle cx="12" cy="12" r="10" />
              <path d="M12 8v8" />
              <path d="M8 12h8" />
            </svg>
            选择图标
          </>
        )}
      </button>
    </Popover>
  )
}
