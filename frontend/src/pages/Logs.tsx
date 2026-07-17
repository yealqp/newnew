import { useEffect, useMemo, useState } from 'react'
import {
  Button,
  Descriptions,
  Drawer,
  Form,
  Input,
  Select,
  Space,
  Table,
  Tabs,
  Tag,
  Typography,
} from 'antd'
import { Search, Eye } from 'lucide-react'
import dayjs from 'dayjs'
import { api, type RequestLog } from '../api/client'
import MarkdownView, { isRoleLang } from '../components/MarkdownView'
import { extractLogContent, extractRawRoleBlocks, extractResponseText, prettyJson } from '../utils/logContent'

export default function Logs() {
  const [list, setList] = useState<RequestLog[]>([])
  const [total, setTotal] = useState(0)
  const [page, setPage] = useState(1)
  const [pageSize, setPageSize] = useState(20)
  const [loading, setLoading] = useState(false)
  const [detail, setDetail] = useState<RequestLog | null>(null)
  const [filters, setFilters] = useState<Record<string, string>>({})
  const [form] = Form.useForm()

  const load = async (p = page, ps = pageSize, f = filters) => {
    setLoading(true)
    try {
      const r = await api.listLogs({ page: p, page_size: ps, ...f })
      setList(r.data.list || [])
      setTotal(r.data.total || 0)
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    load()
  }, [])

  const openDetail = async (id: number) => {
    const r = await api.getLog(id)
    setDetail(r.data)
  }

  const reqExtracted = useMemo(
    () => extractLogContent(detail?.request_body, 'request'),
    [detail?.request_body],
  )
  const respExtracted = useMemo(
    () => extractLogContent(detail?.response_body, 'response'),
    [detail?.response_body],
  )

  const columns = [
    {
      title: '时间',
      dataIndex: 'created_at',
      width: 170,
      render: (v: string) => (
        <span className="mono" style={{ fontSize: 12 }}>
          {dayjs(v).format('YYYY-MM-DD HH:mm:ss')}
        </span>
      ),
    },
    {
      title: '状态',
      dataIndex: 'status',
      width: 90,
      render: (s: string) =>
        s === 'success' ? <Tag color="success">success</Tag> : <Tag color="error">{s}</Tag>,
    },
    { title: '模型', dataIndex: 'model', render: (v: string) => <span className="mono">{v}</span> },
    { title: '渠道', dataIndex: 'channel_name' },
    { title: '令牌', dataIndex: 'token_name' },
    {
      title: 'Token',
      width: 190,
      render: (_: unknown, r: RequestLog) => {
        const prompt = r.prompt_tokens || 0
        const completion = r.completion_tokens || 0
        const cacheRead = r.cache_read_tokens || 0
        const cacheWrite = r.cache_write_tokens || 0
        if (!prompt && !completion && !cacheRead && !cacheWrite) {
          return <span style={{ color: 'var(--text-dim)' }}>-</span>
        }
        return (
          <div className="log-token-cell">
            <div className="log-token-main mono">
              <span title="输入 Token">
                <span className="log-token-label">输入</span>
                {prompt.toLocaleString()}
              </span>
              <span className="log-token-separator">/</span>
              <span title="输出 Token">
                <span className="log-token-label">输出</span>
                {completion.toLocaleString()}
              </span>
            </div>
            <div className="log-token-meta mono">
              {(cacheRead > 0 || cacheWrite > 0) && (
                <span>
                  缓存{cacheRead > 0 ? ` ↓${cacheRead.toLocaleString()}` : ''}
                  {cacheWrite > 0 ? ` ↑${cacheWrite.toLocaleString()}` : ''}
                </span>
              )}
              <span className="log-token-total">总计 {r.total_tokens.toLocaleString()}</span>
            </div>
          </div>
        )
      },
    },
    {
      title: '费用',
      dataIndex: 'cost_rmb',
      width: 110,
      render: (v: number) => <span className="cost">¥{(v || 0).toFixed(6)}</span>,
    },
    {
      title: '耗时',
      width: 150,
      render: (_: unknown, r: RequestLog) => {
        const totalSec = r.duration_ms / 1000
        const speed = r.total_tokens > 0 && totalSec > 0
          ? Math.round(r.total_tokens / totalSec)
          : null
        return (
          <div className="mono" style={{ fontSize: 11, lineHeight: 1.6 }}>
            <div style={{ marginBottom: 2 }}>
              <Tag style={{ margin: 0, padding: '0 6px', fontSize: 10 }}>
                {r.is_stream ? 'stream' : 'non-stream'}
              </Tag>
            </div>
            <span style={{ color: 'var(--text-dim)' }}>TTFT </span>
            <span>{r.first_token_ms ? `${r.first_token_ms}ms` : '-'}</span>
            <br />
            <span style={{ color: 'var(--text-dim)' }}>Total </span>
            <span>{r.duration_ms}ms</span>
            {speed ? (
              <>
                <br />
                <span style={{ color: 'var(--text-dim)' }}>Speed </span>
                <span style={{ color: 'var(--success)' }}>{speed.toLocaleString()} t/s</span>
              </>
            ) : null}
          </div>
        )
      },
    },
    {
      title: '',
      width: 80,
      render: (_: unknown, r: RequestLog) => (
        <Button type="link" size="small" icon={<Eye size={14} />} onClick={() => openDetail(r.id)}>
          详情
        </Button>
      ),
    },
  ]

  return (
    <div>
      <h1 className="page-title">日志</h1>
      <p className="page-desc">请求记录 · Token 用量 · 人民币费用统计</p>

      <Form
        form={form}
        layout="inline"
        style={{ marginBottom: 16, gap: 8, flexWrap: 'wrap' }}
        onFinish={(values) => {
          const f: Record<string, string> = {}
          if (values.model) f.model = values.model
          if (values.status) f.status = values.status
          setFilters(f)
          setPage(1)
          load(1, pageSize, f)
        }}
      >
        <Form.Item name="model">
          <Input placeholder="模型" allowClear className="mono" style={{ width: 180 }} />
        </Form.Item>
        <Form.Item name="status">
          <Select
            allowClear
            placeholder="状态"
            style={{ width: 120 }}
            options={[
              { value: 'success', label: 'success' },
              { value: 'error', label: 'error' },
            ]}
          />
        </Form.Item>
        <Form.Item>
          <Button type="primary" htmlType="submit" icon={<Search size={16} />}>
            筛选
          </Button>
        </Form.Item>
      </Form>

      <Table
        rowKey="id"
        loading={loading}
        columns={columns}
        dataSource={list}
        scroll={{ x: 1100 }}
        pagination={{
          current: page,
          pageSize,
          total,
          showSizeChanger: true,
          onChange: (p, ps) => {
            setPage(p)
            setPageSize(ps)
            load(p, ps)
          },
        }}
      />

      <Drawer
        title="请求详情"
        open={!!detail}
        onClose={() => setDetail(null)}
        width={Math.min(820, typeof window !== 'undefined' ? window.innerWidth : 820)}
        destroyOnHidden
      >
        {detail && (
          <Space direction="vertical" size="large" style={{ width: '100%' }}>
            <Descriptions column={2} size="small" bordered>
              <Descriptions.Item label="Request ID" span={2}>
                <span className="mono">{detail.request_id}</span>
              </Descriptions.Item>
              <Descriptions.Item label="时间">
                {dayjs(detail.created_at).format('YYYY-MM-DD HH:mm:ss')}
              </Descriptions.Item>
              <Descriptions.Item label="状态">
                {detail.status === 'success' ? (
                  <Tag color="success">success</Tag>
                ) : (
                  <Tag color="error">{detail.status}</Tag>
                )}
              </Descriptions.Item>
              <Descriptions.Item label="模型">
                <span className="mono">{detail.model}</span>
              </Descriptions.Item>
              <Descriptions.Item label="上游模型">
                <span className="mono">{detail.upstream_model}</span>
              </Descriptions.Item>
              <Descriptions.Item label="渠道">{detail.channel_name}</Descriptions.Item>
              <Descriptions.Item label="令牌">{detail.token_name}</Descriptions.Item>
              <Descriptions.Item label="Prompt">{detail.prompt_tokens}</Descriptions.Item>
              <Descriptions.Item label="Completion">{detail.completion_tokens}</Descriptions.Item>
              <Descriptions.Item label="Cache Read">{detail.cache_read_tokens}</Descriptions.Item>
              <Descriptions.Item label="Cache Write">{detail.cache_write_tokens}</Descriptions.Item>
              <Descriptions.Item label="费用">
                <span className="cost">¥{(detail.cost_rmb || 0).toFixed(6)}</span>
              </Descriptions.Item>
              <Descriptions.Item label="耗时">
                <span className="mono">{detail.duration_ms} ms</span>
                {detail.is_stream && detail.first_token_ms ? (
                  <span style={{ marginLeft: 12, fontSize: 12, color: 'var(--text-dim)' }}>
                    TTFT {detail.first_token_ms}ms
                    {detail.total_tokens > 0
                      ? ` · ${Math.round(detail.total_tokens / (detail.duration_ms / 1000)).toLocaleString()} t/s`
                      : ''}
                  </span>
                ) : null}
              </Descriptions.Item>
              <Descriptions.Item label="流式">
                {detail.is_stream ? <Tag>stream</Tag> : '非流式'}
              </Descriptions.Item>
              {detail.error_message ? (
                <Descriptions.Item label="错误" span={2}>
                  <Typography.Text type="danger">{detail.error_message}</Typography.Text>
                </Descriptions.Item>
              ) : null}
            </Descriptions>

            <BodyPanel
              title="请求内容"
              kind="request"
              raw={detail.request_body}
              extracted={reqExtracted}
              stream={detail.is_stream}
            />
            <BodyPanel
              title="响应内容"
              kind="response"
              raw={detail.response_body}
              extracted={respExtracted}
              stream={detail.is_stream}
            />

            {detail.detail ? (
              <div>
                <Typography.Text type="secondary">Detail</Typography.Text>
                <pre className="mono log-raw-pre">{prettyJson(detail.detail)}</pre>
              </div>
            ) : null}
          </Space>
        )}
      </Drawer>
    </div>
  )
}

function BodyPanel({
  title,
  kind,
  raw,
  extracted,
  stream,
}: {
  title: string
  kind: 'request' | 'response'
  raw: string
  extracted: ReturnType<typeof extractLogContent>
  stream: boolean
}) {
  const formatHint =
    extracted.format === 'empty'
      ? ''
      : extracted.format.startsWith('stream')
        ? ` · ${extracted.format}${stream ? '' : ' (detected)'}`
        : ` · ${extracted.format}`

  return (
    <div>
      <div style={{ display: 'flex', alignItems: 'baseline', gap: 8, marginBottom: 8 }}>
        <Typography.Text style={{ fontWeight: 500, color: 'var(--foreground)' }}>{title}</Typography.Text>
        <Typography.Text type="secondary" style={{ fontSize: 12 }}>
          {formatHint}
        </Typography.Text>
      </div>
      <Tabs
        size="small"
        items={[
          {
            key: 'md',
            label: 'Markdown',
            children: (
              <MarkdownView
                content={extracted.markdown}
                emptyText="无法解析为可读内容，请查看 Raw 或 Json 标签"
                maxHeight={420}
              />
            ),
          },
          {
            key: 'raw',
            label: 'Raw',
            children:
              kind === 'response' ? <RawResponseView raw={raw} /> : <RawRoleView raw={raw} />,
          },
          {
            key: 'json',
            label: 'Json',
            children: <pre className="mono log-raw-pre">{prettyJson(raw)}</pre>,
          },
        ]}
      />
    </div>
  )
}

function RawResponseView({ raw }: { raw: string }) {
  const text = extractResponseText(raw)
  return (
    <pre className="mono log-raw-pre" style={{ fontSize: 15 }}>
      {text || '(no text content)'}
    </pre>
  )
}

function RawRoleView({ raw }: { raw: string }) {
  const blocks = extractRawRoleBlocks(raw)
  if (!blocks) {
    return (
      <pre className="mono log-raw-pre" style={{ whiteSpace: 'pre-wrap' }}>
        {raw || '(no text content)'}
      </pre>
    )
  }
  return (
    <div className="md-view" style={{ maxHeight: 420 }}>
      {blocks.map((b, i) => (
        <div key={i} className="md-code-block">
          <div className={`md-code-block-lang${isRoleLang(b.role) ? ' md-code-block-lang-role' : ''}`}>
            {b.role}
          </div>
          <pre className="md-raw-content">{b.content}</pre>
        </div>
      ))}
    </div>
  )
}
