import { useEffect, useState } from 'react'
import { Card, Col, Row, Spin, Statistic, DatePicker, Segmented } from 'antd'
import {
  Bar,
  BarChart,
  Area,
  AreaChart,
  CartesianGrid,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
  Legend,
} from 'recharts'
import dayjs, { Dayjs } from 'dayjs'
import { api, type DashboardRangeData } from '../api/client'

const { RangePicker } = DatePicker

type Granularity = 'hour' | 'day' | 'week'

const GRANULARITY_OPTIONS = [
  { label: '小时', value: 'hour' },
  { label: '天', value: 'day' },
  { label: '周', value: 'week' },
]

function fmtNum(n: number) {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(2) + 'M'
  if (n >= 1_000) return (n / 1_000).toFixed(1) + 'K'
  return String(n)
}

function fmtRMB(n: number) {
  return '¥' + (n || 0).toFixed(4)
}

export default function Dashboard() {
  const [data, setData] = useState<DashboardRangeData | null>(null)
  const [loading, setLoading] = useState(true)
  const [range, setRange] = useState<[Dayjs, Dayjs]>([
    dayjs().subtract(7, 'day'),
    dayjs(),
  ])
  const [granularity, setGranularity] = useState<Granularity>('day')

  useEffect(() => {
    setLoading(true)
    const start = range[0].startOf('day').format('YYYY-MM-DD HH:mm:ss')
    const end = range[1].endOf('day').format('YYYY-MM-DD HH:mm:ss')
    api
      .dashboardRange(start, end, granularity)
      .then((r) => setData(r.data))
      .finally(() => setLoading(false))
  }, [range, granularity])

  const timeFmt = granularity === 'hour' ? 'MM-DD HH:mm' : 'YYYY-MM-DD'

  const handleRangeChange = (dates: null | (Dayjs | null)[]) => {
    if (dates && dates[0] && dates[1]) {
      setRange([dates[0], dates[1]])
    }
  }

  if (loading) {
    return (
      <div style={{ textAlign: 'center', padding: 80 }}>
        <Spin size="large" />
      </div>
    )
  }

  const series = data?.series || []
  const distribution = data?.distribution || []

  return (
    <div>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', marginBottom: 16 }}>
        <div>
          <h1 className="page-title">仪表盘</h1>
          <p className="page-desc">用量统计 · Token 与人民币费用（仅统计，不扣费）</p>
        </div>
        <div style={{ display: 'flex', gap: 12, alignItems: 'center', flexWrap: 'wrap' }}>
          <Segmented
            options={GRANULARITY_OPTIONS}
            value={granularity}
            onChange={(v) => setGranularity(v as Granularity)}
          />
          <RangePicker
            value={range}
            onChange={handleRangeChange}
            presets={[
              { label: '今天', value: [dayjs().startOf('day'), dayjs()] },
              { label: '昨天', value: [dayjs().subtract(1, 'day').startOf('day'), dayjs().subtract(1, 'day').endOf('day')] },
              { label: '近 7 天', value: [dayjs().subtract(7, 'day'), dayjs()] },
              { label: '近 30 天', value: [dayjs().subtract(30, 'day'), dayjs()] },
            ]}
          />
        </div>
      </div>

      <Row gutter={[16, 16]}>
        <Col xs={24} sm={12} lg={{ flex: 1 }}>
          <Card>
            <Statistic title="请求数" value={data?.requests || 0} />
          </Card>
        </Col>
        <Col xs={24} sm={12} lg={{ flex: 1 }}>
          <Card>
            <Statistic title="总 Token" value={fmtNum(data?.total_tokens || 0)} />
          </Card>
        </Col>
        <Col xs={24} sm={12} lg={{ flex: 1 }}>
          <Card>
            <div className="stat-label">费用</div>
            <div className="stat-value cost">{fmtRMB(data?.cost_rmb || 0)}</div>
          </Card>
        </Col>
        <Col xs={24} sm={12} lg={{ flex: 1 }}>
          <Card>
            <Statistic title="RPM（请求数 / 分钟）" value={(data?.rpm || 0).toFixed(2)} />
          </Card>
        </Col>
        <Col xs={24} sm={12} lg={{ flex: 1 }}>
          <Card>
            <Statistic title="TPM（Token / 分钟）" value={(data?.tpm || 0).toFixed(2)} />
          </Card>
        </Col>
      </Row>

      <Card title="时间序列" style={{ marginTop: 16 }}>
        <div style={{ width: '100%', height: 320 }}>
          <ResponsiveContainer>
            <AreaChart data={series}>
              <defs>
                <linearGradient id="costGrad" x1="0" y1="0" x2="0" y2="1">
                  <stop offset="0%" stopColor="#e08a6a" stopOpacity={0.35} />
                  <stop offset="100%" stopColor="#e08a6a" stopOpacity={0} />
                </linearGradient>
                <linearGradient id="tokenGrad" x1="0" y1="0" x2="0" y2="1">
                  <stop offset="0%" stopColor="#7aa8d4" stopOpacity={0.35} />
                  <stop offset="100%" stopColor="#7aa8d4" stopOpacity={0} />
                </linearGradient>
              </defs>
              <CartesianGrid strokeDasharray="3 3" stroke="rgba(255,255,255,0.08)" />
              <XAxis
                dataKey="time"
                stroke="#8a8680"
                fontSize={12}
                tickFormatter={(v) => dayjs(v).format(timeFmt)}
              />
              <YAxis yAxisId="left" stroke="#8a8680" fontSize={12} />
              <YAxis yAxisId="right" orientation="right" stroke="#8a8680" fontSize={12} />
              <Tooltip
                contentStyle={{
                  background: '#2a2825',
                  border: '1px solid rgba(255,255,255,0.10)',
                  borderRadius: 10,
                  boxShadow: '0 1px 3px rgba(0,0,0,0.35)',
                  fontFamily: "'Lora', serif",
                }}
                labelStyle={{ color: '#f5f3ee' }}
                labelFormatter={(v) =>
                  granularity === 'week'
                    ? dayjs(v).format('YYYY-MM-DD') + ' 起'
                    : dayjs(v).format(granularity === 'hour' ? 'YYYY-MM-DD HH:mm' : 'YYYY-MM-DD')
                }
              />
              <Legend />
              <Area
                yAxisId="left"
                type="monotone"
                dataKey="cost_rmb"
                name="费用 ¥"
                stroke="#e08a6a"
                fill="url(#costGrad)"
                strokeWidth={2}
              />
              <Area
                yAxisId="right"
                type="monotone"
                dataKey="total_tokens"
                name="Token"
                stroke="#7aa8d4"
                fill="url(#tokenGrad)"
                strokeWidth={2}
              />
            </AreaChart>
          </ResponsiveContainer>
        </div>
      </Card>

      <Card title="渠道 Token 分布" style={{ marginTop: 16 }}>
        <div style={{ width: '100%', height: 360 }}>
          <ResponsiveContainer>
            <BarChart data={distribution}>
              <CartesianGrid strokeDasharray="3 3" stroke="rgba(255,255,255,0.08)" />
              <XAxis dataKey="channel_name" stroke="#8a8680" fontSize={12} angle={-20} textAnchor="end" height={80} />
              <YAxis stroke="#8a8680" fontSize={12} />
              <Tooltip
                contentStyle={{
                  background: '#2a2825',
                  border: '1px solid rgba(255,255,255,0.10)',
                  borderRadius: 10,
                  boxShadow: '0 1px 3px rgba(0,0,0,0.35)',
                  fontFamily: "'Lora', serif",
                }}
                labelStyle={{ color: '#f5f3ee' }}
              />
              <Legend />
              <Bar dataKey="prompt_tokens" name="Prompt Token" fill="#e08a6a" />
              <Bar dataKey="completion_tokens" name="Completion Token" fill="#7aa8d4" />
            </BarChart>
          </ResponsiveContainer>
        </div>
      </Card>
    </div>
  )
}
