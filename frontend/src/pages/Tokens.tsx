import { useEffect, useState } from 'react'
import {
  Button,
  Form,
  Input,
  Modal,
  Popconfirm,
  Select,
  Space,
  Table,
  Tag,
  message,
} from 'antd'
import { Copy, Plus, RefreshCw, Edit, Trash2 } from 'lucide-react'
import { api, type Token } from '../api/client'
import { splitCsv, uniqueModelsFromChannels } from '../utils/models'
import { filterOptionBySearch } from '../utils/format'
import FormDrawer from '../components/FormDrawer'

// 解析令牌的模型限制（JSON 数组或逗号分隔）为字符串数组。
function parseModelLimits(v: string | undefined): string[] {
  if (!v) return []
  const s = v.trim()
  if (!s || s === '[]') return []
  if (s.startsWith('[')) {
    try {
      const arr = JSON.parse(s)
      return Array.isArray(arr) ? arr.filter(Boolean) : []
    } catch {
      return []
    }
  }
  return splitCsv(s)
}

export default function Tokens() {
  const [list, setList] = useState<Token[]>([])
  const [loading, setLoading] = useState(false)
  const [open, setOpen] = useState(false)
  const [saving, setSaving] = useState(false)
  const [editing, setEditing] = useState<Token | null>(null)
  const [form] = Form.useForm()
  const [createdKey, setCreatedKey] = useState<string | null>(null)
  const [modelOptions, setModelOptions] = useState<string[]>([])

  const load = async () => {
    setLoading(true)
    try {
      const r = await api.listTokens()
      const sorted = (r.data || []).slice().sort((a, b) => a.id - b.id)
      setList(sorted)
    } finally {
      setLoading(false)
    }
  }

  // 汇总所有渠道的支持模型，作为模型限制下拉的可选项。
  const loadModelOptions = () => {
    api
      .listChannels()
      .then((r) => setModelOptions(uniqueModelsFromChannels(r.data || [])))
      .catch(() => {
        // 忽略：下拉仍可手动输入
      })
  }

  useEffect(() => {
    load()
    loadModelOptions()
  }, [])

  const copy = async (text: string) => {
    await navigator.clipboard.writeText(text)
    message.success('已复制')
  }

  const openCreate = () => {
    setEditing(null)
    form.resetFields()
    form.setFieldsValue({ status: 1, model_limits: [] })
    setOpen(true)
  }

  const openEdit = (row: Token) => {
    setEditing(row)
    form.setFieldsValue({
      name: row.name,
      status: row.status,
      model_limits: parseModelLimits(row.model_limits),
    })
    setOpen(true)
  }

  const submit = async () => {
    const values = await form.validateFields()
    const limits = (values.model_limits || []) as string[]
    const model_limits = limits.length ? JSON.stringify(limits) : ''
    const payload = { ...values, model_limits }
    setSaving(true)
    try {
      if (editing) {
        await api.updateToken(editing.id, payload)
        message.success('已更新')
      } else {
        const r = await api.createToken(payload)
        setCreatedKey(r.data.key)
        message.success('已创建令牌')
      }
      setOpen(false)
      load()
    } finally {
      setSaving(false)
    }
  }

  const columns = [
    {
      title: 'ID',
      dataIndex: 'id',
      width: 60,
      sorter: (a: Token, b: Token) => a.id - b.id,
      defaultSortOrder: 'ascend' as const,
    },
    { title: '名称', dataIndex: 'name', render: (v: string) => <b>{v}</b> },
    {
      title: 'Key',
      dataIndex: 'key',
      render: (v: string) => (
        <Space>
          <span className="key-chip" title={v}>
            {v}
          </span>
          <Button type="text" size="small" icon={<Copy size={14} />} onClick={() => copy(v)} />
        </Space>
      ),
    },
    {
      title: '模型限制',
      dataIndex: 'model_limits',
      render: (v: string) =>
        !v || v === '[]' ? (
          <Tag>不限</Tag>
        ) : (
          <span className="mono" style={{ color: 'var(--text-muted)' }}>
            {v}
          </span>
        ),
    },
    {
      title: '状态',
      dataIndex: 'status',
      width: 80,
      render: (s: number) => (s === 1 ? <Tag color="success">启用</Tag> : <Tag>禁用</Tag>),
    },
    {
      title: '最近使用',
      dataIndex: 'accessed_at',
      render: (v: string) =>
        v ? (
          <span className="mono" style={{ fontSize: 12 }}>
            {new Date(v).toLocaleString()}
          </span>
        ) : (
          '-'
        ),
    },
    {
      title: '操作',
      width: 220,
      render: (_: unknown, r: Token) => (
        <Space>
          <Button type="link" size="small" icon={<Edit size={14} />} onClick={() => openEdit(r)}>
            编辑
          </Button>
          <Popconfirm
            title="重置 Key？旧 Key 将立即失效"
            onConfirm={async () => {
              const res = await api.resetTokenKey(r.id)
              setCreatedKey(res.data.key)
              message.success('已重置')
              load()
            }}
          >
            <Button type="link" size="small" icon={<RefreshCw size={14} />}>
              重置
            </Button>
          </Popconfirm>
          <Popconfirm
            title="确认删除？"
            onConfirm={async () => {
              await api.deleteToken(r.id)
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
          <h1 className="page-title">令牌</h1>
          <p className="page-desc">客户端 API Key · 仅鉴权与日志归属 · 无额度限制</p>
        </div>
        <Button type="primary" icon={<Plus size={16} />} onClick={openCreate}>
          新建令牌
        </Button>
      </div>

      <Table rowKey="id" loading={loading} columns={columns} dataSource={list} pagination={{ pageSize: 20 }} />

      <FormDrawer
        title={editing ? '编辑令牌' : '新建令牌'}
        open={open}
        onClose={() => setOpen(false)}
        onSave={submit}
        saving={saving}
        width={440}
      >
        <Form form={form} layout="vertical" requiredMark="optional">
          <Form.Item name="name" label="名称" rules={[{ required: true, message: '请输入名称' }]}>
            <Input placeholder="例如 本地开发" />
          </Form.Item>
          <Form.Item name="status" label="状态">
            <Select
              options={[
                { value: 1, label: '启用' },
                { value: 0, label: '禁用' },
              ]}
            />
          </Form.Item>
          <Form.Item name="model_limits" label="模型限制" extra="留空不限；从渠道模型中选择，也可输入自定义模型名">
            <Select
              mode="tags"
              className="mono"
              placeholder="留空表示不限制"
              allowClear
              tokenSeparators={[',']}
              options={modelOptions.map((m) => ({ label: m, value: m }))}
              filterOption={filterOptionBySearch}
            />
          </Form.Item>
        </Form>
      </FormDrawer>

      <Modal
        title="令牌 Key"
        open={!!createdKey}
        onCancel={() => setCreatedKey(null)}
        footer={[
          <Button key="copy" type="primary" onClick={() => createdKey && copy(createdKey)}>
            复制
          </Button>,
          <Button key="ok" onClick={() => setCreatedKey(null)}>
            关闭
          </Button>,
        ]}
      >
        <p style={{ color: 'var(--text-muted)' }}>请妥善保存，可随时在列表中再次复制。</p>
        <div
          className="mono"
          style={{
            background: 'var(--surface-2)',
            border: '1px solid var(--border)',
            borderRadius: 4,
            padding: 12,
            wordBreak: 'break-all',
          }}
        >
          {createdKey}
        </div>
      </Modal>
    </div>
  )
}
