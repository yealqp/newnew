import { useEffect, useMemo, useState, type ReactNode } from 'react'
import {
  Button,
  Drawer,
  Form,
  Input,
  InputNumber,
  Modal,
  Popconfirm,
  Select,
  Space,
  Switch,
  Table,
  Tag,
  Tooltip,
  message,
} from 'antd'
import {
  CloudDownload,
  Edit,
  Trash2,
  Plus,
  Zap,
  Check,
  X,
  AlertCircle,
} from 'lucide-react'
import { OpenAI, Anthropic } from '@lobehub/icons'
import { api, type Channel, type ModelPrice } from '../api/client'
import IconPicker from '../components/IconPicker'
import { LobeIcon } from '../utils/lobeIcons'

type PricingMap = Record<string, ModelPrice>

// Upstream 协议类型的展示元信息（value 仍为后端约定的 openai/claude）。
const TYPE_META: Record<string, { label: string; color: string; icon: ReactNode }> = {
  openai: { label: 'OpenAI', color: 'blue', icon: <OpenAI size={14} /> },
  claude: { label: 'Anthropic', color: 'purple', icon: <Anthropic size={14} /> },
}

type PricingRow = {
  key: string
  model: string
  input: number
  output: number
  cache_read: number
  cache_write: number
}

function parsePricing(s: string): PricingMap {
  try {
    return s ? JSON.parse(s) : {}
  } catch {
    return {}
  }
}

function pricingToRows(pricing: PricingMap): PricingRow[] {
  return Object.entries(pricing).map(([model, p], i) => ({
    key: `${model}-${i}`,
    model,
    input: p.input ?? 0,
    output: p.output ?? 0,
    cache_read: p.cache_read ?? 0,
    cache_write: p.cache_write ?? 0,
  }))
}

function rowsToPricing(rows: PricingRow[]): PricingMap {
  const map: PricingMap = {}
  for (const r of rows) {
    const name = r.model.trim()
    if (!name) continue
    map[name] = {
      input: Number(r.input) || 0,
      output: Number(r.output) || 0,
      cache_read: Number(r.cache_read) || 0,
      cache_write: Number(r.cache_write) || 0,
    }
  }
  return map
}

function emptyRow(): PricingRow {
  return {
    key: `new-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`,
    model: '',
    input: 0,
    output: 0,
    cache_read: 0,
    cache_write: 0,
  }
}

type MappingRow = {
  key: string
  request: string
  upstream: string
}

function parseMapping(s: string): Record<string, string> {
  try {
    return s ? JSON.parse(s) : {}
  } catch {
    return {}
  }
}

function mappingToRows(m: Record<string, string>): MappingRow[] {
  return Object.entries(m).map(([request, upstream], i) => ({
    key: `map-${request}-${i}`,
    request,
    upstream: upstream ?? '',
  }))
}

function rowsToMapping(rows: MappingRow[]): Record<string, string> {
  const map: Record<string, string> = {}
  for (const r of rows) {
    const from = r.request.trim()
    const to = r.upstream.trim()
    if (!from || !to) continue
    map[from] = to
  }
  return map
}

function emptyMappingRow(): MappingRow {
  return {
    key: `map-new-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`,
    request: '',
    upstream: '',
  }
}

// 模型映射右侧「上游模型」选择器：下拉源为该渠道的「支持模型」列表；
// 允许输入列表外的模型名（通过下拉底部的「使用自定义」入口），由父级决定是否加入支持模型。
function UpstreamModelSelect({
  value,
  supportedModels,
  onChange,
}: {
  value: string
  supportedModels: string[]
  onChange: (v: string) => void
}) {
  const [search, setSearch] = useState('')
  const searchTrim = search.trim()
  const showCustom = searchTrim !== '' && !supportedModels.includes(searchTrim)
  const commit = (v: string) => {
    onChange(v)
    setSearch('')
  }
  return (
    <Select
      className="mono"
      showSearch
      allowClear
      placeholder="选择上游模型（可选）"
      value={value || undefined}
      searchValue={search}
      onSearch={setSearch}
      options={supportedModels.map((m) => ({ label: m, value: m }))}
      filterOption={(input, opt) =>
        String(opt?.value ?? '').toLowerCase().includes(input.toLowerCase())
      }
      onChange={(v) => commit(v ?? '')}
      onInputKeyDown={(e) => {
        if (e.key === 'Enter' && showCustom) {
          e.preventDefault()
          commit(searchTrim)
        }
      }}
      notFoundContent={<span style={{ color: 'var(--text-dim)', fontSize: 12 }}>无匹配模型，回车可用作自定义</span>}
      popupRender={(menu) => (
        <>
          {menu}
          {showCustom && (
            <div style={{ padding: 8, borderTop: '1px solid var(--border-soft)' }}>
              <Button
                size="small"
                type="link"
                icon={<Plus size={14} />}
                style={{ paddingLeft: 0 }}
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => commit(searchTrim)}
              >
                使用自定义：“{searchTrim}”
              </Button>
            </div>
          )}
        </>
      )}
    />
  )
}

function formatTokens(n: number): string {
  if (!n) return '-'
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(2) + 'M'
  if (n >= 1_000) return (n / 1_000).toFixed(1) + 'K'
  return String(n)
}

function formatResponseTime(timeMs?: number): string {
  if (!timeMs) return '未测试'
  return timeMs < 1000 ? `${Math.round(timeMs)}ms` : `${(timeMs / 1000).toFixed(2)}s`
}

function responseTimeColor(timeMs?: number): string | undefined {
  if (!timeMs) return undefined
  if (timeMs <= 1000) return 'success'
  if (timeMs <= 2000) return 'warning'
  return 'error'
}

function formatRelativeTestTime(timestamp?: number): string {
  if (!timestamp) return '-'
  const diffSeconds = timestamp - Date.now() / 1000
  const absSeconds = Math.abs(diffSeconds)
  const formatter = new Intl.RelativeTimeFormat('zh-CN', { numeric: 'always', style: 'narrow' })
  if (absSeconds < 60) return formatter.format(Math.round(diffSeconds), 'second')
  if (absSeconds < 3600) return formatter.format(Math.round(diffSeconds / 60), 'minute')
  if (absSeconds < 86400) return formatter.format(Math.round(diffSeconds / 3600), 'hour')
  if (absSeconds < 2592000) return formatter.format(Math.round(diffSeconds / 86400), 'day')
  if (absSeconds < 31536000) return formatter.format(Math.round(diffSeconds / 2592000), 'month')
  return formatter.format(Math.round(diffSeconds / 31536000), 'year')
}

export default function Channels() {
  const [list, setList] = useState<Channel[]>([])
  const [loading, setLoading] = useState(false)
  const [open, setOpen] = useState(false)
  const [saving, setSaving] = useState(false)
  const [editing, setEditing] = useState<Channel | null>(null)
  const [form] = Form.useForm()
  const supportedModels: string[] = Form.useWatch('models', form) || []
  const [pricingRows, setPricingRows] = useState<PricingRow[]>([])
  const [mappingRows, setMappingRows] = useState<MappingRow[]>([])
  const [fetchingModels, setFetchingModels] = useState(false)
  const [modelPickerOpen, setModelPickerOpen] = useState(false)
  const [upstreamModels, setUpstreamModels] = useState<string[]>([])
  const [selectedModels, setSelectedModels] = useState<string[]>([])
  const [modelFilter, setModelFilter] = useState('')
  const [testingId, setTestingId] = useState<number | null>(null)

  const load = async () => {
    setLoading(true)
    try {
      const r = await api.listChannels()
      const sorted = (r.data || []).sort((a, b) => a.id - b.id)
      setList(sorted)
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    load()
  }, [])

  const openCreate = () => {
    setEditing(null)
    form.resetFields()
    form.setFieldsValue({
      type: 'openai',
      status: 1,
      weight: 1,
      priority: 0,
      base_url: '',
      full_url: false,
      models: [],
    })
    setPricingRows([])
    setMappingRows([])
    setOpen(true)
  }

  const openEdit = async (row: Channel) => {
    setEditing(row)
    try {
      const r = await api.getChannel(row.id)
      const ch = r.data
      const modelsArr = ch.models
        ? ch.models.split(',').map((s: string) => s.trim()).filter(Boolean)
        : []
      form.setFieldsValue({ ...ch, models: modelsArr })
      setPricingRows(pricingToRows(parsePricing(ch.pricing || '{}')))
      setMappingRows(mappingToRows(parseMapping(ch.model_mapping || '{}')))
    } catch {
      const modelsArr = row.models
        ? row.models.split(',').map((s: string) => s.trim()).filter(Boolean)
        : []
      form.setFieldsValue({ ...row, models: modelsArr })
      setPricingRows(pricingToRows(parsePricing(row.pricing || '{}')))
      setMappingRows(mappingToRows(parseMapping(row.model_mapping || '{}')))
    }
    setOpen(true)
  }

  const updateRow = (key: string, patch: Partial<PricingRow>) => {
    setPricingRows((rows) => rows.map((r) => (r.key === key ? { ...r, ...patch } : r)))
  }

  const removeRow = (key: string) => {
    setPricingRows((rows) => rows.filter((r) => r.key !== key))
  }

  const addRow = () => {
    setPricingRows((rows) => [...rows, emptyRow()])
  }

  const updateMappingRow = (key: string, patch: Partial<MappingRow>) => {
    setMappingRows((rows) => rows.map((r) => (r.key === key ? { ...r, ...patch } : r)))
  }

  const removeMappingRow = (key: string) => {
    setMappingRows((rows) => rows.filter((r) => r.key !== key))
  }

  const addMappingRow = () => {
    setMappingRows((rows) => [...rows, emptyMappingRow()])
  }

  const fetchUpstreamModels = async () => {
    try {
      await form.validateFields(['base_url', 'type'])
    } catch {
      message.warning('请先填写 URL 与上游格式')
      return
    }
    const base_url = form.getFieldValue('base_url')
    const type = form.getFieldValue('type')
    const full_url = !!form.getFieldValue('full_url')
    const api_key = form.getFieldValue('api_key') || ''
    if (!api_key && !editing) {
      message.warning('请先填写 API Key')
      return
    }
    setFetchingModels(true)
    try {
      const r = await api.fetchUpstreamModels({
        base_url,
        api_key,
        type,
        full_url,
        channel_id: editing?.id,
      })
      const models = r.data.models || []
      if (!models.length) {
        message.warning('上游未返回模型')
        return
      }
      setUpstreamModels(models)
      const current: string[] = form.getFieldValue('models') || []
      setSelectedModels(current.filter((m) => models.includes(m)))
      setModelFilter('')
      setModelPickerOpen(true)
    } catch {
      // toast by interceptor
    } finally {
      setFetchingModels(false)
    }
  }

  const applySelectedModels = () => {
    if (!selectedModels.length) {
      message.warning('请至少选择一个模型')
      return
    }
    form.setFieldsValue({ models: selectedModels })
    setModelPickerOpen(false)
    const current = rowsToPricing(pricingRows)
    const rows: PricingRow[] = selectedModels.map((model, i) => {
      const p = current[model]
      return {
        key: `pick-${model}-${i}`,
        model,
        input: p?.input ?? 0,
        output: p?.output ?? 0,
        cache_read: p?.cache_read ?? 0,
        cache_write: p?.cache_write ?? 0,
      }
    })
    setPricingRows(rows)
    message.success(`已填入 ${selectedModels.length} 个模型`)
  }

  const filteredUpstream = useMemo(() => {
    const q = modelFilter.trim().toLowerCase()
    if (!q) return upstreamModels
    return upstreamModels.filter((m) => m.toLowerCase().includes(q))
  }, [upstreamModels, modelFilter])

  const syncFromModels = () => {
    const models: string[] = form.getFieldValue('models') || []
    if (!models.length) {
      message.warning('请先选择支持模型')
      return
    }
    const current = rowsToPricing(pricingRows)
    const next: PricingRow[] = models.map((model, i) => {
      const p = current[model]
      return {
        key: `sync-${model}-${i}`,
        model,
        input: p?.input ?? 0,
        output: p?.output ?? 0,
        cache_read: p?.cache_read ?? 0,
        cache_write: p?.cache_write ?? 0,
      }
    })
    for (const r of pricingRows) {
      if (r.model && !models.includes(r.model.trim())) {
        next.push(r)
      }
    }
    setPricingRows(next)
    message.success('已从支持模型同步')
  }

  const submit = async () => {
    const values = await form.validateFields()
    const pricing = rowsToPricing(pricingRows)
    if (!Object.keys(pricing).length) {
      message.error('请至少配置一个模型定价')
      return
    }
    let models = (values.models || []) as string[]
    if (!models.length) {
      message.error('请选择支持模型')
      return
    }

    // 校验：映射两侧模型若不在「支持模型」列表，提醒用户是否加入。
    // 请求模型（左侧）不在列表时无法被路由匹配，影响更大，单独提示。
    const mapping = rowsToMapping(mappingRows)
    const q = (m: string) => `“${m}”`
    const reqMissing = Object.keys(mapping).filter((m) => !models.includes(m))
    const upMissing = Array.from(
      new Set(Object.values(mapping).filter((m) => m && !models.includes(m))),
    ).filter((m) => !reqMissing.includes(m))
    const missing = Array.from(new Set([...reqMissing, ...upMissing]))
    if (missing.length) {
      const add = await new Promise<boolean>((resolve) => {
        Modal.confirm({
          title: '加入支持模型？',
          content: (
            <div>
              {reqMissing.length > 0 && (
                <p style={{ marginBottom: 8 }}>
                  请求模型 {reqMissing.map(q).join('、')} 不在支持模型列表中，不加入将无法被路由匹配。
                </p>
              )}
              {upMissing.length > 0 && (
                <p style={{ marginBottom: 8 }}>
                  上游模型 {upMissing.map(q).join('、')} 不在支持模型列表中。
                </p>
              )}
              <p style={{ margin: 0 }}>是否将以上模型加入支持模型？</p>
            </div>
          ),
          okText: '加入',
          cancelText: '暂不加入',
          onOk: () => resolve(true),
          onCancel: () => resolve(false),
        })
      })
      if (add) {
        models = [...models, ...missing]
        form.setFieldsValue({ models })
      }
    }

    const payload = {
      ...values,
      models: models.join(','),
      pricing: JSON.stringify(pricing),
      model_mapping: JSON.stringify(mapping),
    }
    setSaving(true)
    try {
      if (editing) {
        await api.updateChannel(editing.id, payload)
        message.success('已更新')
      } else {
        await api.createChannel(payload)
        message.success('已创建')
      }
      setOpen(false)
      load()
    } finally {
      setSaving(false)
    }
  }

  const testChannel = async (id: number) => {
    setTestingId(id)
    try {
      const r = await api.testChannel(id)
      const t = r.data.response_time
      const dur = t >= 1000 ? `${(t / 1000).toFixed(2)}s` : `${Math.round(t)}ms`
      message.success(`测试成功：${dur}`)
      load()
    } catch (err: any) {
      const msg = err?.response?.data?.message || '测试失败'
      message.error(msg)
    } finally {
      setTestingId(null)
    }
  }

  const columns = [
    {
      title: 'ID',
      dataIndex: 'id',
      width: 60,
      sorter: (a: Channel, b: Channel) => a.id - b.id,
      defaultSortOrder: 'ascend' as const,
    },
    {
      title: '名称',
      dataIndex: 'name',
      render: (v: string, r: Channel) => (
        <Space>
          <LobeIcon id={r.icon} size={16} />
          <span style={{ fontWeight: 600 }}>{v}</span>
          {r.remark ? <span style={{ color: 'var(--text-dim)', fontSize: 12 }}>{r.remark}</span> : null}
        </Space>
      ),
    },
    {
      title: '类型',
      dataIndex: 'type',
      width: 100,
      render: (t: string) => {
        const meta = TYPE_META[t]
        return (
          <Tag color={meta?.color} icon={meta?.icon} style={{ display: 'inline-flex', alignItems: 'center', gap: 4 }}>
            {meta?.label ?? t}
          </Tag>
        )
      },
    },
    {
      title: '总计 Token',
      dataIndex: 'total_tokens',
      width: 120,
      sorter: (a: Channel, b: Channel) => (a.total_tokens || 0) - (b.total_tokens || 0),
      render: (v: number) => (
        <span className="mono" style={{ color: 'var(--primary)' }}>
          {formatTokens(v)}
        </span>
      ),
    },
    {
      title: '优先级/权重',
      width: 110,
      render: (_: unknown, r: Channel) => (
        <span className="mono">
          {r.priority}/{r.weight}
        </span>
      ),
    },
    {
      title: '状态',
      dataIndex: 'status',
      width: 80,
      render: (s: number) =>
        s === 1 ? (
          <Tag color="success" icon={<Check size={12} />}>
            启用
          </Tag>
        ) : (
          <Tag icon={<X size={12} />}>禁用</Tag>
        ),
    },
    {
      title: '响应',
      dataIndex: 'response_time',
      width: 100,
      sorter: (a: Channel, b: Channel) => (a.response_time || 0) - (b.response_time || 0),
      render: (v?: number) => (
        <Tag color={responseTimeColor(v)} className="mono" style={{ margin: 0 }}>
          {formatResponseTime(v)}
        </Tag>
      ),
    },
    {
      title: '上次测试',
      dataIndex: 'test_time',
      width: 110,
      sorter: (a: Channel, b: Channel) => (a.test_time || 0) - (b.test_time || 0),
      render: (v?: number) =>
        v ? (
          <Tooltip title={new Date(v * 1000).toLocaleString('zh-CN')}>
            <Tag className="mono" style={{ margin: 0, cursor: 'default' }}>
              {formatRelativeTestTime(v)}
            </Tag>
          </Tooltip>
        ) : (
          <span style={{ color: 'var(--text-dim)' }}>-</span>
        ),
    },
    {
      title: '操作',
      width: 160,
      render: (_: unknown, r: Channel) => (
        <Space>
          <Button
            type="link"
            size="small"
            icon={<Zap size={14} />}
            loading={testingId === r.id}
            onClick={() => testChannel(r.id)}
          >
            测速
          </Button>
          <Button type="link" size="small" icon={<Edit size={14} />} onClick={() => openEdit(r)}>
            编辑
          </Button>
          <Popconfirm
            title="确认删除？"
            onConfirm={async () => {
              await api.deleteChannel(r.id)
              message.success('已删除')
              load()
            }}
          >
            <Button type="link" size="small" danger icon={<Trash2 size={14} />}>
              删除
            </Button>
          </Popconfirm>
        </Space>
      ),
    },
  ]

  return (
    <div>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start' }}>
        <div>
          <h1 className="page-title">渠道</h1>
          <p className="page-desc">上游提供商 · 协议格式 OpenAI / Anthropic · 价格单位 ¥/1M tokens</p>
        </div>
        <Button type="primary" icon={<Plus size={16} />} onClick={openCreate}>
          新建渠道
        </Button>
      </div>

      <Table
        rowKey="id"
        loading={loading}
        columns={columns}
        dataSource={list}
        pagination={{ pageSize: 20 }}
        scroll={{ x: 1200 }}
      />

      <Drawer
        title={editing ? '编辑渠道' : '新建渠道'}
        open={open}
        onClose={() => setOpen(false)}
        width={Math.min(720, typeof window !== 'undefined' ? window.innerWidth : 720)}
        destroyOnHidden
        placement="right"
        styles={{
          body: { paddingBottom: 80 },
        }}
        footer={
          <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
            <Button onClick={() => setOpen(false)}>取消</Button>
            <Button type="primary" loading={saving} onClick={submit}>
              保存
            </Button>
          </div>
        }
      >
        <Form form={form} layout="vertical" requiredMark="optional">
          <div style={{ display: 'flex', gap: 10, alignItems: 'flex-start' }}>
            <Form.Item name="name" label="名称" rules={[{ required: true, message: '请输入名称' }]} style={{ flex: 1, marginBottom: 0 }}>
              <Input placeholder="例如 DeepSeek 官方" />
            </Form.Item>
            <Form.Item name="icon" label="图标" style={{ marginBottom: 0 }}>
              <IconPicker />
            </Form.Item>
          </div>

          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
            <Form.Item name="type" label="上游格式" rules={[{ required: true }]}>
              <Select
                options={[
                  {
                    value: 'openai',
                    label: (
                      <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}>
                        <OpenAI size={14} /> OpenAI
                      </span>
                    ),
                  },
                  {
                    value: 'claude',
                    label: (
                      <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}>
                        <Anthropic size={14} /> Anthropic
                      </span>
                    ),
                  },
                ]}
              />
            </Form.Item>
            <Form.Item name="status" label="状态">
              <Select
                options={[
                  { value: 1, label: '启用' },
                  { value: 0, label: '禁用' },
                ]}
              />
            </Form.Item>
            <Form.Item name="priority" label="优先级">
              <InputNumber style={{ width: '100%' }} />
            </Form.Item>
            <Form.Item name="weight" label="权重">
              <InputNumber min={1} style={{ width: '100%' }} />
            </Form.Item>
          </div>

          <div style={{ display: 'grid', gridTemplateColumns: '1fr auto', gap: 12, alignItems: 'start' }}>
            <Form.Item
              noStyle
              shouldUpdate={(prev, cur) => prev.full_url !== cur.full_url || prev.type !== cur.type}
            >
              {() => {
                const full = !!form.getFieldValue('full_url')
                const t = form.getFieldValue('type') || 'openai'
                const ph = full
                  ? t === 'claude'
                    ? 'https://example.com/custom/v1/messages'
                    : 'https://example.com/custom/v1/chat/completions'
                  : 'https://api.openai.com'
                return (
                  <Form.Item
                    name="base_url"
                    label={full ? '完整端点 URL' : 'Base URL'}
                    rules={[{ required: true, message: '请填写 URL' }]}
                    extra={
                      full
                        ? '已开启完整 URL：将原样作为上游请求地址，不再拼接 /v1/...'
                        : '默认拼接 /v1/chat/completions 或 /v1/messages'
                    }
                    style={{ marginBottom: 0 }}
                  >
                    <Input placeholder={ph} className="mono" />
                  </Form.Item>
                )
              }}
            </Form.Item>
            <Form.Item
              name="full_url"
              label="完整 URL"
              valuePropName="checked"
              style={{ marginBottom: 0, minWidth: 88 }}
              tooltip="开启后，上方输入即为 OpenAI/Anthropic 的完整请求端点"
            >
              <Switch checkedChildren="开" unCheckedChildren="关" />
            </Form.Item>
          </div>

          <Form.Item
            name="api_key"
            label="API Key"
            rules={editing ? [] : [{ required: true, message: '请输入 API Key' }]}
            extra={editing ? '含 *** 表示脱敏，不修改请保持原样' : '多 Key 用换行分隔'}
            style={{ marginTop: 16 }}
          >
            <Input.TextArea rows={2} className="mono" placeholder="sk-..." />
          </Form.Item>

          <Form.Item
            label="支持模型"
            required
            extra="从上游获取或手动输入模型名称（回车添加）"
          >
            <div style={{ display: 'flex', gap: 8 }}>
              <Form.Item
                name="models"
                noStyle
                rules={[{ required: true, message: '请选择支持模型' }]}
              >
                <Select
                  mode="tags"
                  placeholder="输入模型名称回车添加"
                  className="mono"
                  style={{ flex: 1 }}
                  tokenSeparators={[',']}
                />
              </Form.Item>
              <Button
                icon={<CloudDownload size={16} />}
                loading={fetchingModels}
                onClick={fetchUpstreamModels}
              >
                获取
              </Button>
            </div>
          </Form.Item>

          <div style={{ marginBottom: 16 }}>
            <div style={{ fontWeight: 600, marginBottom: 4 }}>模型映射</div>
            <p className="pricing-unit-hint">左侧为客户端请求模型名，右侧为转发到上游的模型名（可选）</p>
            <div className="mapping-editor">
              <div className="mapping-editor-head">
                <span>请求模型</span>
                <span className="mapping-arrow" />
                <span>上游模型</span>
                <span />
              </div>
              <div className="mapping-editor-body">
                {mappingRows.length === 0 ? (
                  <div style={{ padding: 14, color: 'var(--text-dim)', fontSize: 13, textAlign: 'center' }}>
                    无映射时按同名转发
                  </div>
                ) : (
                  mappingRows.map((row) => (
                    <div className="mapping-editor-row" key={row.key}>
                      <Input
                        className="mono"
                        placeholder="client-model"
                        value={row.request}
                        onChange={(e) => updateMappingRow(row.key, { request: e.target.value })}
                      />
                      <span className="mapping-arrow">→</span>
                      <UpstreamModelSelect
                        value={row.upstream}
                        supportedModels={supportedModels}
                        onChange={(v) => updateMappingRow(row.key, { upstream: v })}
                      />
                      <Button
                        type="text"
                        danger
                        icon={<Trash2 size={14} />}
                        onClick={() => removeMappingRow(row.key)}
                      />
                    </div>
                  ))
                )}
              </div>
              <div className="mapping-editor-actions">
                <span />
                <Button size="small" type="dashed" icon={<Plus size={14} />} onClick={addMappingRow}>
                  添加映射
                </Button>
              </div>
            </div>
          </div>

          <div style={{ marginBottom: 16 }}>
            <div style={{ fontWeight: 600, marginBottom: 4 }}>模型定价</div>
            <p className="pricing-unit-hint">单位：¥ / 1M tokens · 每行一个模型</p>
            <div className="pricing-editor">
              <div className="pricing-editor-head">
                <span>模型</span>
                <span>输入</span>
                <span>输出</span>
                <span>缓存读</span>
                <span>缓存写</span>
                <span />
              </div>
              <div className="pricing-editor-body">
                {pricingRows.length === 0 ? (
                  <div style={{ padding: 16, color: 'var(--text-dim)', fontSize: 13, textAlign: 'center' }}>
                    暂无定价，点击下方添加
                  </div>
                ) : (
                  pricingRows.map((row) => (
                    <div className="pricing-editor-row" key={row.key}>
                      <Input
                        className="mono"
                        placeholder="model-id"
                        value={row.model}
                        onChange={(e) => updateRow(row.key, { model: e.target.value })}
                      />
                      <InputNumber
                        min={0}
                        step={0.01}
                        controls={false}
                        value={row.input}
                        onChange={(v) => updateRow(row.key, { input: Number(v) || 0 })}
                      />
                      <InputNumber
                        min={0}
                        step={0.01}
                        controls={false}
                        value={row.output}
                        onChange={(v) => updateRow(row.key, { output: Number(v) || 0 })}
                      />
                      <InputNumber
                        min={0}
                        step={0.01}
                        controls={false}
                        value={row.cache_read}
                        onChange={(v) => updateRow(row.key, { cache_read: Number(v) || 0 })}
                      />
                      <InputNumber
                        min={0}
                        step={0.01}
                        controls={false}
                        value={row.cache_write}
                        onChange={(v) => updateRow(row.key, { cache_write: Number(v) || 0 })}
                      />
                      <Button
                        type="text"
                        danger
                        icon={<Trash2 size={14} />}
                        onClick={() => removeRow(row.key)}
                      />
                    </div>
                  ))
                )}
              </div>
              <div className="pricing-editor-actions">
                <Button size="small" onClick={syncFromModels}>
                  从支持模型同步
                </Button>
                <Button size="small" type="dashed" icon={<Plus size={14} />} onClick={addRow}>
                  添加模型
                </Button>
              </div>
            </div>
          </div>

          <Form.Item name="remark" label="备注">
            <Input />
          </Form.Item>
        </Form>
      </Drawer>

      <Modal
        title={`选择模型（共 ${upstreamModels.length} 个）`}
        open={modelPickerOpen}
        onCancel={() => setModelPickerOpen(false)}
        width={560}
        destroyOnHidden
        footer={
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
            <Space>
              <Button size="small" onClick={() => setSelectedModels([...filteredUpstream])}>
                全选当前
              </Button>
              <Button size="small" onClick={() => setSelectedModels([])}>
                清空
              </Button>
              <span style={{ color: 'var(--text-dim)', fontSize: 12 }}>
                已选 {selectedModels.length}
              </span>
            </Space>
            <Space>
              <Button onClick={() => setModelPickerOpen(false)}>取消</Button>
              <Button type="primary" onClick={applySelectedModels}>
                填入支持模型
              </Button>
            </Space>
          </div>
        }
      >
        <Input
          allowClear
          placeholder="筛选模型 ID…"
          className="mono"
          value={modelFilter}
          onChange={(e) => setModelFilter(e.target.value)}
          style={{ marginBottom: 12 }}
          prefix={<AlertCircle size={14} style={{ color: 'var(--text-dim)' }} />}
        />
        <div
          style={{
            maxHeight: 360,
            overflow: 'auto',
            border: '1px solid var(--border)',
            borderRadius: 8,
            padding: '8px 12px',
            background: 'var(--surface-2)',
          }}
        >
          {filteredUpstream.length === 0 ? (
            <div style={{ color: 'var(--text-dim)', padding: 16, textAlign: 'center' }}>无匹配模型</div>
          ) : (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
              {filteredUpstream.map((m) => (
                <label
                  key={m}
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    gap: 8,
                    cursor: 'pointer',
                    padding: '4px 8px',
                    borderRadius: 4,
                    background: selectedModels.includes(m) ? 'var(--primary-soft)' : 'transparent',
                  }}
                >
                  <input
                    type="checkbox"
                    checked={selectedModels.includes(m)}
                    onChange={(e) => {
                      if (e.target.checked) {
                        setSelectedModels([...selectedModels, m])
                      } else {
                        setSelectedModels(selectedModels.filter((s) => s !== m))
                      }
                    }}
                    style={{ accentColor: 'var(--primary)' }}
                  />
                  <span className="mono" style={{ fontSize: 12 }}>
                    {m}
                  </span>
                </label>
              ))}
            </div>
          )}
        </div>
      </Modal>
    </div>
  )
}
