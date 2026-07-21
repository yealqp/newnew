import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Button, Input, Select, Spin, Typography, message } from 'antd'
import {
  Copy,
  Edit,
  RefreshCw,
  Trash2,
  Send,
  StopCircle,
  Plus,
  Menu,
  ChevronDown,
  ChevronRight,
  MessageSquare,
  Check,
} from 'lucide-react'
import MarkdownView from '../components/MarkdownView'
import { api, type Conversation } from '../api/client'
import { getToken } from '../utils/auth'
import { uniqueModelsFromChannels } from '../utils/models'
import { filterOptionBySearch } from '../utils/format'
import { iterateSSEData } from '../utils/logContent'

// ---- Types ----

interface ChatMsg {
  role: 'user' | 'assistant'; content: string; id?: number
}

interface ThinkParse {
  visible: string; reasoning: string; unclosed: boolean
}

// ---- Utilities ----

function parseThink(content: string): ThinkParse {
  const parts: string[] = []
  const thinks: string[] = []
  let pos = 0
  while (true) {
    const open = content.indexOf('<think>', pos)
    if (open === -1) { parts.push(content.slice(pos)); break }
    parts.push(content.slice(pos, open))
    const close = content.indexOf('</think>', open + 7)
    if (close === -1) {
      thinks.push(content.slice(open + 7))
      return { visible: parts.join('').trim(), reasoning: thinks.join('\n\n'), unclosed: true }
    }
    thinks.push(content.slice(open + 7, close))
    pos = close + 8
  }
  return { visible: parts.join('').trim(), reasoning: thinks.join('\n\n').trim(), unclosed: false }
}

// ---- Message Component ----

function PlayMsg({
  msg,
  isLast,
  isGenerating,
  onRegenerate,
  onEdit,
  onDelete,
}: {
  msg: ChatMsg
  isLast: boolean
  isGenerating: boolean
  onRegenerate?: () => void
  onEdit?: (content: string) => void
  onDelete?: () => void
}) {
  const [editing, setEditing] = useState(false)
  const [editText, setEditText] = useState(msg.content)
  const [copied, setCopied] = useState(false)
  const [reasoningOpen, setReasoningOpen] = useState(true)

  const parsed = useMemo(() => parseThink(msg.content), [msg.content])
  const isAssistant = msg.role === 'assistant'
  const isStreaming = isLast && isGenerating && isAssistant
  const showReasoning = isAssistant && parsed.reasoning.length > 0

  const handleCopy = async () => {
    await navigator.clipboard.writeText(msg.content)
    setCopied(true); setTimeout(() => setCopied(false), 1500)
  }

  const handleSaveEdit = () => {
    onEdit?.(editText); setEditing(false)
  }

  const content =
    msg.role === 'user' ? (
      <MarkdownView content={msg.content} emptyText="*(empty)*" maxHeight="none" />
    ) : (
      <>
        {showReasoning && (
          <div style={{ marginBottom: 8 }}>
            <button
              type="button"
              onClick={() => setReasoningOpen((v) => !v)}
              style={{
                display: 'inline-flex', alignItems: 'center', gap: 4,
                border: 'none', background: 'rgba(224,138,106,0.1)', borderRadius: 6,
                padding: '4px 10px', cursor: 'pointer', color: 'var(--primary)',
                fontSize: 12, fontFamily: 'inherit', fontWeight: 500,
              }}
            >
              {reasoningOpen ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
              思考 {reasoningOpen ? '' : `(${parsed.reasoning.length}字)`}
            </button>
            {reasoningOpen && (
              <div
                style={{
                  marginTop: 6, padding: '8px 12px',
                  background: 'rgba(224,138,106,0.04)',
                  borderLeft: '2px solid var(--primary)',
                  borderRadius: '0 6px 6px 0',
                  fontSize: 13, color: 'var(--text-dim)',
                  lineHeight: 1.6, whiteSpace: 'pre-wrap',
                }}
              >
                {parsed.reasoning}
              </div>
            )}
          </div>
        )}
        <MarkdownView
          content={parsed.visible || (isStreaming ? '' : msg.content)}
          emptyText={isStreaming ? undefined : '*(empty)*'}
          maxHeight="none"
        />
      </>
    )

  return (
    <div
      className="playground-msg"
      style={{
        display: 'flex',
        flexDirection: 'column',
        alignItems: isAssistant ? 'flex-start' : 'flex-end',
        width: '100%',
        marginBottom: 4,
        padding: '10px 0',
        position: 'relative',
      }}
    >
      {/* Message bubble */}
      <div
        style={{
          ...(isAssistant
            ? { maxWidth: '78ch', width: '100%' }
            : {
                maxWidth: 'min(85%, 72ch)',
                background: 'var(--surface-2)',
                border: '1px solid var(--border)',
                borderRadius: '16px 16px 4px 16px',
                padding: '10px 14px',
              }),
        }}
      >
        {editing ? (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
            <Input.TextArea
              value={editText}
              onChange={(e) => setEditText(e.target.value)}
              rows={3}
              style={{ fontFamily: 'var(--font-body)', fontSize: 14 }}
            />
            <div style={{ display: 'flex', gap: 6, justifyContent: 'flex-end' }}>
              <Button size="small" onClick={() => setEditing(false)}>取消</Button>
              <Button size="small" type="primary" onClick={handleSaveEdit}>保存</Button>
            </div>
          </div>
        ) : (
          content
        )}
      </div>

      {/* Timestamp + actions */}
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 8,
          marginTop: 4,
          fontSize: 11,
          color: 'var(--text-dim)',
          opacity: 0,
          transition: 'opacity 0.15s',
        }}
        className="playground-msg-actions"
        onPointerEnter={(e) => { e.currentTarget.style.opacity = '1' }}
        onPointerLeave={(e) => { e.currentTarget.style.opacity = '0' }}
      >
        {isAssistant && showReasoning && (
          <span>{parsed.reasoning.length}字推理</span>
        )}
        {isStreaming && <span style={{ color: 'var(--primary)' }}>生成中…</span>}
        {!editing && (
          <>
            <button type="button" onClick={handleCopy} title="复制" className="icon-btn">
              {copied ? <Check size={12} /> : <Copy size={12} />}
            </button>
            {isAssistant && onRegenerate && (
              <button type="button" onClick={onRegenerate} title="重新生成" className="icon-btn">
                <RefreshCw size={12} />
              </button>
            )}
            <button type="button" onClick={() => { setEditText(msg.content); setEditing(true) }} title="编辑" className="icon-btn">
              <Edit size={12} />
            </button>
            <button type="button" onClick={onDelete} title="删除" className="icon-btn">
              <Trash2 size={12} />
            </button>
          </>
        )}
      </div>
    </div>
  )
}

// ---- Main Page ----

export default function Playground() {
  const [conversations, setConversations] = useState<Conversation[]>([])
  const [activeId, setActiveId] = useState<number | null>(null)
  const [messages, setMessages] = useState<ChatMsg[]>([])
  const [input, setInput] = useState('')
  const [loading, setLoading] = useState(false)
  const [streamingContent, setStreamingContent] = useState('')
  const [models, setModels] = useState<string[]>([])
  const [model, setModel] = useState('')
  const [sidebarOpen, setSidebarOpen] = useState(true)
  const abortRef = useRef<AbortController | null>(null)
  const bottomRef = useRef<HTMLDivElement>(null)
  const msgContainerRef = useRef<HTMLDivElement>(null)

  // Load models from channels
  useEffect(() => {
    api.listChannels().then((r) => setModels(uniqueModelsFromChannels(r.data || []))).catch(() => {})
  }, [])

  const loadConversations = useCallback(async () => {
    try {
      const r = await api.listConversations()
      setConversations(r.data || [])
    } catch { /* ignore */ }
  }, [])
  useEffect(() => { loadConversations() }, [loadConversations])

  // Load messages when active conversation changes
  useEffect(() => {
    if (!activeId) { setMessages([]); return }
    setStreamingContent('')
    api.listConversationMessages(activeId)
      .then((r) => setMessages((r.data || []).map((m) => ({ role: m.role as ChatMsg['role'], content: m.content, id: m.id }))))
      .catch(() => setMessages([]))
  }, [activeId])

  const activeConv = conversations.find((c) => c.id === activeId)
  useEffect(() => { if (activeConv?.model) setModel(activeConv.model) }, [activeConv?.model])

  const scrollBottom = () =>
    setTimeout(() => bottomRef.current?.scrollIntoView({ behavior: 'smooth' }), 50)
  useEffect(() => { scrollBottom() }, [messages, streamingContent])

  const createConversation = async () => {
    const r = await api.createConversation({
      title: '新对话', model: model || models[0] || '',
    })
    const conv = r.data
    setConversations((prev) => [conv, ...prev])
    setActiveId(conv.id); setMessages([]); setStreamingContent('')
  }

  const deleteConversation = async (id: number, e: React.MouseEvent) => {
    e.stopPropagation()
    await api.deleteConversation(id)
    setConversations((prev) => prev.filter((c) => c.id !== id))
    if (activeId === id) setActiveId(null)
  }

  const stopStream = useCallback(() => {
    abortRef.current?.abort(); abortRef.current = null
    if (streamingContent) {
      setMessages((prev) => [...prev, { role: 'assistant', content: streamingContent }])
      // 中止时也持久化已生成的部分
      if (activeId) {
        api.addConversationMessage(activeId, {
          role: 'assistant', content: streamingContent,
        }).then(loadConversations).catch(() => {})
      }
      setStreamingContent('')
    }
    setLoading(false)
  }, [streamingContent, activeId, loadConversations])

  const send = useCallback(async (overrides?: { msg?: string; msgs?: ChatMsg[] }) => {
    const text = overrides?.msg ?? input.trim()
    if (!text || !model || loading || !activeId) {
      if (!activeId) message.warning('请先新建或选择一个对话')
      return
    }
    if (!overrides?.msg) setInput('')
    setLoading(true)

    const userMsg: ChatMsg = { role: 'user', content: text }
    const baseMsgs = overrides?.msgs ?? messages
    const newMessages = [...baseMsgs, userMsg]
    setMessages(newMessages)
    setStreamingContent('')

    const controller = new AbortController()
    abortRef.current = controller

    try {
      // 先落库用户消息（服务端会自动改标题、刷新会话时间）
      await api.addConversationMessage(activeId, {
        role: 'user', content: text,
      }).catch(() => {})

      // 直接请求网关自己的 OpenAI 兼容中继接口：渠道选择与 OpenAI/Claude
      // 格式转换全部由后端中继层完成，任意类型上游都可用
      const res = await fetch('/v1/chat/completions', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${getToken()}` },
        body: JSON.stringify({
          model,
          messages: newMessages.map((m) => ({ role: m.role, content: m.content })),
          stream: true,
          temperature: 0.7,
        }),
        signal: controller.signal,
      })
      if (!res.ok) {
        const err = await res.json().catch(() => ({ message: res.statusText }))
        const msg = err.error?.message || err.message || '请求失败'
        setMessages((prev) => [...prev, { role: 'assistant', content: `**Error:** ${msg}` }])
        setLoading(false); loadConversations(); return
      }
      const reader = res.body?.getReader()
      if (!reader) { setLoading(false); return }
      const decoder = new TextDecoder()
      let buffer = '', fullContent = ''
      while (true) {
        const { done, value } = await reader.read()
        if (done) break
        buffer += decoder.decode(value, { stream: true })
        const lines = buffer.split('\n')
        buffer = lines.pop() || ''
        for (const d of iterateSSEData(lines.join('\n'))) {
          try {
            const p = JSON.parse(d); const delta = p.choices?.[0]?.delta?.content || ''
            if (delta) { fullContent += delta; setStreamingContent(fullContent) }
          } catch { /* skip */ }
        }
      }
      setMessages((prev) => [...prev, { role: 'assistant', content: fullContent }])
      setStreamingContent('')
      if (fullContent) {
        await api.addConversationMessage(activeId, {
          role: 'assistant', content: fullContent,
        }).catch(() => {})
      }
    } catch (err: any) {
      if (err.name === 'AbortError') return
      setMessages((prev) => [...prev, { role: 'assistant', content: `**Error:** ${err.message || '网络错误'}` }])
    } finally {
      setLoading(false); abortRef.current = null; loadConversations()
    }
  }, [input, model, messages, loading, activeId, loadConversations])

  const handleEdit = (i: number) => (newContent: string) => {
    const updated = [...messages]; updated[i] = { ...updated[i], content: newContent }
    setMessages(updated)
    // Re-send with updated history
    const allBefore = updated.slice(0, i)
    const editedMsg = updated[i]
    if (editedMsg.role === 'assistant') return // just edit the content
    // If editing user message, resend from that point
    setMessages(allBefore)
    setInput(editedMsg.content)
  }

  const handleRegenerate = (i: number) => () => {
    const msgs = messages.slice(0, i) // take all messages before this one
    setMessages(msgs)
    const lastUser = [...msgs].reverse().find((m) => m.role === 'user')
    if (lastUser) send({ msg: lastUser.content, msgs })
  }

  const handleDelete = (i: number) => () => {
    setMessages((prev) => prev.filter((_, idx) => idx !== i))
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); send() }
  }

  const allMessages = [...messages, ...(streamingContent ? [{ role: 'assistant' as const, content: streamingContent }] : [])]

  return (
    <div style={{ display: 'flex', height: 'calc(100vh - 64px)', margin: -24, width: 'calc(100% + 48px)' }}>
      {/* Sidebar */}
      <div style={{
        width: sidebarOpen ? 240 : 0, overflow: 'hidden', flexShrink: 0,
        display: 'flex', flexDirection: 'column',
        borderRight: sidebarOpen ? '1px solid var(--border)' : 'none',
        transition: 'width 0.2s',
      }}>
        <div style={{ padding: '8px 10px', borderBottom: '1px solid var(--border)' }}>
          <Button block type="dashed" icon={<Plus size={14} />} onClick={createConversation}>新建对话</Button>
        </div>
        <div style={{ flex: 1, overflow: 'auto', padding: 6 }}>
          {conversations.length === 0 ? (
            <div style={{ color: 'var(--text-dim)', fontSize: 12, textAlign: 'center', padding: 16 }}>暂无对话</div>
          ) : (
            conversations.map((conv) => (
              <div key={conv.id} onClick={() => setActiveId(conv.id)}
                style={{
                  display: 'flex', alignItems: 'center', gap: 6, padding: '8px 10px', borderRadius: 6,
                  cursor: 'pointer', marginBottom: 2,
                  background: activeId === conv.id ? 'var(--primary-soft)' : 'transparent',
                  color: activeId === conv.id ? 'var(--foreground)' : 'var(--text-dim)', fontSize: 13,
                }}
                onPointerEnter={(e) => { if (activeId !== conv.id) e.currentTarget.style.background = 'var(--muted)' }}
                onPointerLeave={(e) => { if (activeId !== conv.id) e.currentTarget.style.background = 'transparent' }}
              >
                <MessageSquare size={14} style={{ flexShrink: 0 }} />
                <span style={{ flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                  {conv.title || '新对话'}
                </span>
                <span style={{ fontSize: 11, color: 'var(--text-dim)', flexShrink: 0 }}>{conv.message_count || ''}</span>
                <button type="button" onClick={(e) => deleteConversation(conv.id, e)}
                  className="icon-btn" style={{ opacity: 0.4 }}>
                  <Trash2 size={12} />
                </button>
              </div>
            ))
          )}
        </div>
      </div>

      {/* Main chat area */}
      <div style={{ flex: 1, display: 'flex', flexDirection: 'column', minWidth: 0 }}>
        {/* Header */}
        <div style={{
          display: 'flex', alignItems: 'center', gap: 10, padding: '12px 20px',
          borderBottom: '1px solid var(--border)',
        }}>
          <button type="button" onClick={() => setSidebarOpen((v) => !v)}
            className="icon-btn" style={{ padding: 4 }}>
            <Menu size={16} />
          </button>
          <Typography.Text style={{ fontWeight: 600, color: 'var(--foreground)', fontSize: 15 }}>游乐场</Typography.Text>
          {activeConv && <Typography.Text style={{ fontSize: 12, color: 'var(--text-dim)' }}>{activeConv.title}</Typography.Text>}
        </div>

        {/* Messages */}
        <div ref={msgContainerRef} style={{ flex: 1, overflow: 'auto', padding: '0 20px' }}>
          <div style={{ maxWidth: 800, margin: '0 auto' }}>
            {!activeId ? (
              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', height: '100%', minHeight: 400, color: 'var(--text-dim)', fontSize: 14 }}>
                <div style={{ textAlign: 'center', padding: 40 }}>
                  <div style={{ fontSize: 48, marginBottom: 12, opacity: 0.3 }}>✦</div>
                  <div style={{ fontWeight: 600, marginBottom: 4 }}>OpenGate 游乐场</div>
                  <div style={{ fontSize: 13 }}>新建一个对话开始测试你的渠道</div>
                </div>
              </div>
            ) : allMessages.length === 0 && !loading ? (
              <div style={{ textAlign: 'center', padding: '60px 20px', color: 'var(--text-dim)' }}>
                <div style={{ fontSize: 40, marginBottom: 12, opacity: 0.25 }}>💬</div>
                <div style={{ fontWeight: 500, marginBottom: 4 }}>开始新对话</div>
                <div style={{ fontSize: 13 }}>选择一个模型，在下方输入消息</div>
              </div>
            ) : (
              allMessages.map((msg, i) => (
                <PlayMsg
                  key={`${i}-${msg.role}-${msg.content.slice(0, 20)}`}
                  msg={msg}
                  isLast={i === allMessages.length - 1}
                  isGenerating={loading}
                  onRegenerate={msg.role === 'assistant' && !loading ? handleRegenerate(i) : undefined}
                  onEdit={!loading ? handleEdit(i) : undefined}
                  onDelete={!loading ? handleDelete(i) : undefined}
                />
              ))
            )}
            {loading && !streamingContent && (
              <div style={{ padding: 30, textAlign: 'center' }}><Spin size="small" /></div>
            )}
            <div ref={bottomRef} />
          </div>
        </div>

        {/* Input area */}
        <div style={{ borderTop: '1px solid var(--border)', padding: '12px 20px 16px' }}>
          <div style={{ maxWidth: 800, margin: '0 auto' }}>
            <div style={{
              border: '1px solid var(--border)', borderRadius: 12, overflow: 'hidden',
              background: 'var(--card)', boxShadow: '0 18px 60px -32px rgba(0,0,0,0.5)',
            }}>
              <Input.TextArea
                value={input}
                onChange={(e) => setInput(e.target.value)}
                onKeyDown={handleKeyDown}
                placeholder="输入消息，Enter 发送，Shift+Enter 换行"
                rows={3}
                disabled={loading || !activeId}
                style={{
                  fontFamily: 'var(--font-body)', fontSize: 14, resize: 'none',
                  border: 'none', background: 'transparent', padding: '12px 16px 8px',
                  boxShadow: 'none', outline: 'none',
                }}
                className="playground-input"
              />
              <div style={{
                display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8,
                padding: '6px 10px 6px 14px',
                borderTop: '1px solid var(--border)',
                background: 'var(--surface-2)',
              }}>
                <Select
                  showSearch
                  className="mono"
                  placeholder="选择模型"
                  value={model || undefined}
                  onChange={(v) => setModel(v)}
                  style={{ width: 'min(240px, 50%)' }}
                  filterOption={filterOptionBySearch}
                  options={models.map((m) => ({ label: m, value: m }))}
                  notFoundContent={null}
                  size="small"
                />
                <Button
                  type={loading ? 'default' : 'primary'}
                  icon={loading ? <StopCircle size={16} /> : <Send size={16} />}
                  onClick={loading ? stopStream : () => send()}
                  style={{
                    ...(loading ? { borderColor: 'var(--destructive)', color: 'var(--destructive)' } : {}),
                    fontWeight: 500,
                  }}
                >
                  {loading ? '停止' : '发送'}
                </Button>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}
