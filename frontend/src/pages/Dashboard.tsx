import { useEffect, useMemo, useState } from 'react'
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
  PieChart,
  Pie,
  Cell,
  LineChart,
  Line,
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

type ModelTab = 'trend' | 'proportion' | 'top'
type DistDimension = 'model' | 'channel'
type DistMetric = 'token' | 'cost'
type DistChartType = 'bar' | 'area'

const MODEL_COLORS = [
  '#e08a6a', '#7aa8d4', '#8fba6e', '#d4a04a',
  '#c98ad4', '#7ad4b4', '#d47a7a', '#9ad47a',
  '#a8b8d4', '#d4b48a',
]

const TOOLTIP_STYLE = {
  background: '#2a2825',
  border: '1px solid rgba(255,255,255,0.10)',
  borderRadius: 10,
  boxShadow: '0 1px 3px rgba(0,0,0,0.35)',
  fontFamily: "'Lora', serif",
}

function shortLabel(name: string, max = 22) {
  const s = name || 'unknown'
  return s.length > max ? s.slice(0, max) + '…' : s
}

function timeLabelFormatter(v: any, granularity: Granularity) {
  return granularity === 'week'
    ? dayjs(v).format('YYYY-MM-DD') + ' 起'
    : dayjs(v).format(granularity === 'hour' ? 'YYYY-MM-DD HH:mm' : 'YYYY-MM-DD')
}

const EMPTY_BOX = (height = 360) => (
  <div
    style={{
      display: 'grid',
      placeItems: 'center',
      height,
      color: 'var(--muted-foreground)',
      fontSize: 13,
    }}
  >
    当前时间范围内无数据
  </div>
)

export default function Dashboard() {
  const [data, setData] = useState<DashboardRangeData | null>(null)
  const [loading, setLoading] = useState(true)
  const [range, setRange] = useState<[Dayjs, Dayjs]>([
    dayjs().subtract(7, 'day'),
    dayjs(),
  ])
  const [granularity, setGranularity] = useState<Granularity>('day')
  const [modelTab, setModelTab] = useState<ModelTab>('trend')
  const [distDimension, setDistDimension] = useState<DistDimension>('model')
  const [distMetric, setDistMetric] = useState<DistMetric>('token')
  const [distChartType, setDistChartType] = useState<DistChartType>('area')

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
  const colorOf = (i: number) => MODEL_COLORS[i % MODEL_COLORS.length]
  const TOP_N = 8

  // ===== 模型调用分析 =====
  const modelStats = useMemo(() => {
    const all = data?.model_stats || []
    if (all.length <= TOP_N) return all
    const top = all.slice(0, TOP_N)
    const rest = all.slice(TOP_N)
    const restAgg = rest.reduce(
      (acc, r) => {
        acc.count += r.count
        acc.prompt_tokens += r.prompt_tokens
        acc.completion_tokens += r.completion_tokens
        acc.total_tokens += r.total_tokens
        acc.cost_rmb += r.cost_rmb
        return acc
      },
      { model: '其他', count: 0, prompt_tokens: 0, completion_tokens: 0, total_tokens: 0, cost_rmb: 0 },
    )
    return [...top, restAgg]
  }, [data?.model_stats])

  const modelTrend = useMemo(() => {
    const raw = data?.model_series || []
    const topModels = new Set(modelStats.map((m) => m.model))
    const byTime = new Map<string, Record<string, number | string>>()
    const times: string[] = []
    for (const r of raw) {
      const model = topModels.has(r.model) ? r.model : '其他'
      if (!byTime.has(r.time)) {
        byTime.set(r.time, { time: r.time })
        times.push(r.time)
      }
      const row = byTime.get(r.time)!
      row[model] = ((row[model] as number) || 0) + r.count
    }
    return times.map((t) => byTime.get(t)!)
  }, [data?.model_series, modelStats])

  // ===== 消耗分布 =====
  const metricVal = (r: any) =>
    distMetric === 'token' ? Number(r.total_tokens || 0) : Number(r.cost_rmb || 0)
  const dimName = (r: any) =>
    distDimension === 'model' ? (r.model || 'unknown') : (r.channel_name || 'unknown')

  const distKeys = useMemo(() => {
    const source = distDimension === 'model' ? (data?.model_stats || []) : (data?.distribution || [])
    const sorted = [...source].sort((a, b) => metricVal(b) - metricVal(a))
    const names = sorted.slice(0, TOP_N).map(dimName)
    if (sorted.length > TOP_N) names.push('其他')
    return names
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [data, distDimension, distMetric])

  const distAreaData = useMemo(() => {
    const rows = distDimension === 'model' ? (data?.model_series || []) : (data?.channel_series || [])
    const topSet = new Set(distKeys)
    const byTime = new Map<string, Record<string, number | string>>()
    const times: string[] = []
    for (const r of rows) {
      const key = topSet.has(dimName(r)) ? dimName(r) : '其他'
      if (!byTime.has(r.time)) {
        byTime.set(r.time, { time: r.time })
        times.push(r.time)
      }
      const row = byTime.get(r.time)!
      row[key] = ((row[key] as number) || 0) + metricVal(r)
    }
    return times.map((t) => byTime.get(t)!)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [data, distDimension, distMetric, distKeys])

  const distTotal = useMemo(() => {
    const source = distDimension === 'model' ? (data?.model_stats || []) : (data?.distribution || [])
    return source.reduce((s, r) => s + metricVal(r), 0)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [data, distDimension, distMetric])

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

  const distTotalLabel =
    distMetric === 'token' ? fmtNum(distTotal) + ' tokens' : fmtRMB(distTotal)

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

      <Card style={{ marginTop: 16 }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12, flexWrap: 'wrap', gap: 8 }}>
          <div style={{ fontWeight: 500, fontSize: 15 }}>
            消耗分布
            <span style={{ marginLeft: 10, fontSize: 12, color: 'var(--muted-foreground)' }}>
              共 {distTotalLabel} · 按{distDimension === 'model' ? '模型' : '渠道'} {distKeys.length} 项
            </span>
          </div>
          <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
            <Segmented
              size="small"
              value={distDimension}
              onChange={(v) => setDistDimension(v as DistDimension)}
              options={[
                { label: '模型', value: 'model' },
                { label: '渠道', value: 'channel' },
              ]}
            />
            <Segmented
              size="small"
              value={distMetric}
              onChange={(v) => setDistMetric(v as DistMetric)}
              options={[
                { label: 'Token', value: 'token' },
                { label: '花费', value: 'cost' },
              ]}
            />
            <Segmented
              size="small"
              value={distChartType}
              onChange={(v) => setDistChartType(v as DistChartType)}
              options={[
                { label: '面积图', value: 'area' },
                { label: '柱状图', value: 'bar' },
              ]}
            />
          </div>
        </div>
        <div style={{ width: '100%', height: 360 }}>
          {distAreaData.length === 0 || distKeys.length === 0 ? (
            EMPTY_BOX()
          ) : (
            <ResponsiveContainer>
              {distChartType === 'area' ? (
                <AreaChart data={distAreaData}>
                  <CartesianGrid strokeDasharray="3 3" stroke="rgba(255,255,255,0.08)" />
                  <XAxis
                    dataKey="time"
                    stroke="#8a8680"
                    fontSize={12}
                    tickFormatter={(v) => dayjs(v).format(timeFmt)}
                  />
                  <YAxis stroke="#8a8680" fontSize={12} />
                  <Tooltip
                    contentStyle={TOOLTIP_STYLE}
                    labelStyle={{ color: '#f5f3ee' }}
                    labelFormatter={(v) => timeLabelFormatter(v, granularity)}
                    formatter={(value: any, name: any) => [
                      distMetric === 'token'
                        ? `${Number(value).toLocaleString()} tokens`
                        : fmtRMB(Number(value)),
                      String(name ?? ''),
                    ]}
                  />
                  <Legend formatter={(v: any) => shortLabel(String(v ?? ''))} />
                  {distKeys.map((k, i) => (
                    <Area
                      key={k}
                      type="monotone"
                      dataKey={k}
                      stackId="1"
                      stroke={colorOf(i)}
                      fill={colorOf(i)}
                      fillOpacity={0.55}
                      strokeWidth={1.5}
                    />
                  ))}
                </AreaChart>
              ) : (
                <BarChart data={distAreaData}>
                  <CartesianGrid strokeDasharray="3 3" stroke="rgba(255,255,255,0.08)" />
                  <XAxis
                    dataKey="time"
                    stroke="#8a8680"
                    fontSize={12}
                    tickFormatter={(v) => dayjs(v).format(timeFmt)}
                  />
                  <YAxis stroke="#8a8680" fontSize={12} />
                  <Tooltip
                    contentStyle={TOOLTIP_STYLE}
                    labelStyle={{ color: '#f5f3ee' }}
                    labelFormatter={(v) => timeLabelFormatter(v, granularity)}
                    formatter={(value: any, name: any) => [
                      distMetric === 'token'
                        ? `${Number(value).toLocaleString()} tokens`
                        : fmtRMB(Number(value)),
                      String(name ?? ''),
                    ]}
                  />
                  <Legend formatter={(v: any) => shortLabel(String(v ?? ''))} />
                  {distKeys.map((k, i) => (
                    <Bar key={k} dataKey={k} stackId="1" fill={colorOf(i)} />
                  ))}
                </BarChart>
              )}
            </ResponsiveContainer>
          )}
        </div>
      </Card>

      <Card style={{ marginTop: 16 }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12, flexWrap: 'wrap', gap: 8 }}>
          <div style={{ fontWeight: 500, fontSize: 15 }}>
            模型调用分析
            <span style={{ marginLeft: 10, fontSize: 12, color: 'var(--muted-foreground)' }}>
              共 {modelStats.reduce((s, m) => s + m.count, 0).toLocaleString()} 次调用 · {modelStats.length} 个模型
            </span>
          </div>
          <Segmented
            size="small"
            options={[
              { label: '趋势', value: 'trend' },
              { label: '占比', value: 'proportion' },
              { label: '排行', value: 'top' },
            ]}
            value={modelTab}
            onChange={(v) => setModelTab(v as ModelTab)}
          />
        </div>
        <div style={{ width: '100%', height: 360 }}>
          {modelStats.length === 0 ? (
            EMPTY_BOX()
          ) : (
            <ResponsiveContainer>
              {modelTab === 'trend' ? (
                <LineChart data={modelTrend}>
                  <CartesianGrid strokeDasharray="3 3" stroke="rgba(255,255,255,0.08)" />
                  <XAxis
                    dataKey="time"
                    stroke="#8a8680"
                    fontSize={12}
                    tickFormatter={(v) => dayjs(v).format(timeFmt)}
                  />
                  <YAxis stroke="#8a8680" fontSize={12} />
                  <Tooltip
                    contentStyle={TOOLTIP_STYLE}
                    labelStyle={{ color: '#f5f3ee' }}
                    labelFormatter={(v) => timeLabelFormatter(v, granularity)}
                  />
                  <Legend />
                  {modelStats.map((m, i) => (
                    <Line
                      key={m.model}
                      type="monotone"
                      dataKey={m.model}
                      stroke={colorOf(i)}
                      strokeWidth={2}
                      dot={false}
                      activeDot={{ r: 4 }}
                    />
                  ))}
                </LineChart>
              ) : modelTab === 'proportion' ? (
                <PieChart>
                  <Pie
                    data={modelStats}
                    dataKey="count"
                    nameKey="model"
                    cx="50%"
                    cy="50%"
                    outerRadius={120}
                    innerRadius={60}
                    paddingAngle={1}
                    label={({ name, percent }: any) =>
                      `${shortLabel(String(name ?? ''))} ${(percent != null ? percent * 100 : 0).toFixed(0)}%`
                    }
                    labelLine={false}
                  >
                    {modelStats.map((m, i) => (
                      <Cell key={m.model} fill={colorOf(i)} />
                    ))}
                  </Pie>
                  <Tooltip
                    contentStyle={TOOLTIP_STYLE}
                    labelStyle={{ color: '#f5f3ee' }}
                    formatter={(value: any, name: any) => [`${Number(value).toLocaleString()} 次`, String(name ?? '')]}
                  />
                  <Legend formatter={(v: any) => shortLabel(String(v ?? ''))} />
                </PieChart>
              ) : (
                <BarChart data={modelStats} layout="vertical" margin={{ left: 8, right: 24 }}>
                  <CartesianGrid strokeDasharray="3 3" stroke="rgba(255,255,255,0.08)" horizontal={false} />
                  <XAxis type="number" stroke="#8a8680" fontSize={12} />
                  <YAxis
                    type="category"
                    dataKey="model"
                    stroke="#8a8680"
                    fontSize={11}
                    width={150}
                    tickFormatter={(v: any) => shortLabel(String(v ?? ''), 18)}
                  />
                  <Tooltip
                    contentStyle={TOOLTIP_STYLE}
                    labelStyle={{ color: '#f5f3ee' }}
                    formatter={(value: any) => [`${Number(value).toLocaleString()} 次`, '调用次数']}
                  />
                  <Bar dataKey="count" name="调用次数" radius={[0, 4, 4, 0]}>
                    {modelStats.map((m, i) => (
                      <Cell key={m.model} fill={colorOf(i)} />
                    ))}
                  </Bar>
                </BarChart>
              )}
            </ResponsiveContainer>
          )}
        </div>
      </Card>
    </div>
  )
}
