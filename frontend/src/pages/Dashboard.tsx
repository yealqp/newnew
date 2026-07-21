import { useEffect, useMemo, useState, useRef } from 'react'
import { Card, Col, Row, Spin, Statistic, DatePicker, Segmented } from 'antd'
import { VChart } from '@visactor/react-vchart'
import type { ISpec } from '@visactor/vchart'
import dayjs, { Dayjs } from 'dayjs'
import { api, type DashboardRangeData } from '../api/client'
import { formatCostRMB, formatTokenCount } from '../utils/format'

const { RangePicker } = DatePicker

type Granularity = 'hour' | 'day' | 'week' | 'month'
type DistDimension = 'model' | 'channel'
type DistMetric = 'token' | 'cost'
type DistChartType = 'bar' | 'area'
type ModelTab = 'trend' | 'proportion' | 'top'

const GRANULARITY_OPTIONS = [
  { label: '时', value: 'hour' },
  { label: '日', value: 'day' },
  { label: '周', value: 'week' },
  { label: '月', value: 'month' },
]

const VCHART_OPTION = { mode: 'desktop-browser' } as const
const MAX_CHART_TREND_POINTS = 7
const TOP_N = 8

const MODEL_COLORS = [
  '#e08a6a', '#7aa8d4', '#8fba6e', '#d4a04a',
  '#c98ad4', '#7ad4b4', '#d47a7a', '#9ad47a',
  '#a8b8d4', '#d4b48a', '#6DC8EC', '#9270CA',
  '#FF9D4D', '#269A99', '#FF99C3', '#5D7092',
  '#F6BD16', '#E8684A', '#5AD8A6', '#5B8FF9',
]

const fmtNum = (n: number) => formatTokenCount(n)
const fmtRMB = (n: number) => formatCostRMB(n, 4)

let themeManagerPromise: Promise<(typeof import('@visactor/vchart'))['ThemeManager']> | null = null

// ---- distribution chart specs ----

type SeriesRow = Record<string, unknown>
type StatsRow = Record<string, unknown>

interface DistSpecs {
  spec_area: Record<string, unknown> | null
  spec_bar: Record<string, unknown> | null
  totalDisplay: string
  distKeys: string[]
}

function buildDistSpecs(
  series: SeriesRow[],
  stats: StatsRow[],
  dimensionKey: string,
  metricKey: string,
  granularity: Granularity,
): DistSpecs {
  const otherLabel = '其他'
  const formatVal = metricKey === 'cost_rmb' ? fmtRMB : (n: number) => n.toLocaleString()

  if (!series.length || !stats.length) {
    return { spec_area: null, spec_bar: null, totalDisplay: formatVal(0), distKeys: [] }
  }

  const dimTotals = new Map<string, number>()
  for (const s of stats) {
    const name = String(s[dimensionKey] || 'unknown')
    dimTotals.set(name, (dimTotals.get(name) || 0) + (Number(s[metricKey]) || 0))
  }

  const sortedDims = [...dimTotals.entries()].sort((a, b) => b[1] - a[1])
  const topDims = sortedDims.slice(0, TOP_N).map(([name]) => name)
  const topSet = new Set(topDims)

  const timeMap = new Map<string, Map<string, number>>()
  const allTimes = new Set<string>()
  for (const row of series) {
    const time = String(row.time || '')
    const name = String(row[dimensionKey] || 'unknown')
    const val = Number(row[metricKey]) || 0
    allTimes.add(time)
    if (!timeMap.has(time)) timeMap.set(time, new Map())
    const dimMap = timeMap.get(time)!
    dimMap.set(name, (dimMap.get(name) || 0) + val)
  }

  let times = [...allTimes].sort()
  if (times.length > 0 && times.length < MAX_CHART_TREND_POINTS) {
    const last = dayjs(times[times.length - 1])
    const unit = granularity === 'hour' ? 'hour' : granularity === 'week' ? 'week' : granularity === 'month' ? 'month' : 'day'
    const fmt = granularity === 'hour' ? 'YYYY-MM-DD HH:00' : granularity === 'month' ? 'YYYY-MM' : 'YYYY-MM-DD'
    const padded: string[] = []
    for (let i = MAX_CHART_TREND_POINTS - 1; i >= 0; i--) {
      const t = last.subtract(i, unit).format(fmt)
      padded.push(t)
      if (!timeMap.has(t)) timeMap.set(t, new Map())
    }
    times = padded
  }

  type DistRow = { Time: string; Model: string; Usage: number; rawValue: number; TimeSum: number }

  const barValues: DistRow[] = []
  for (const time of times) {
    const dimMap = timeMap.get(time) || new Map()
    let timeSum = 0
    const entries: { name: string; val: number }[] = []
    for (const [name, val] of dimMap) {
      timeSum += val
      entries.push({ name, val })
    }
    entries.sort((a, b) => b.val - a.val)
    for (const { name, val } of entries) {
      barValues.push({ Time: time, Model: name, Usage: val, rawValue: val, TimeSum: timeSum })
    }
  }

  const areaValues: DistRow[] = []
  for (const time of times) {
    const dimMap = timeMap.get(time) || new Map()
    const buckets = new Map<string, { raw: number }>()
    let timeSum = 0
    for (const [name, val] of dimMap) {
      timeSum += val
      const key = topSet.has(name) ? name : otherLabel
      const prev = buckets.get(key) || { raw: 0 }
      buckets.set(key, { raw: prev.raw + val })
    }
    for (const [name, { raw }] of buckets) {
      areaValues.push({ Time: time, Model: name, Usage: raw, rawValue: raw, TimeSum: timeSum })
    }
  }
  areaValues.sort((a, b) => a.Time.localeCompare(b.Time))

  const colorDomain = [...topDims]
  if (sortedDims.length > TOP_N) colorDomain.push(otherLabel)
  const colorRange = MODEL_COLORS.slice(0, colorDomain.length)
  const color = { type: 'ordinal' as const, domain: colorDomain, range: colorRange }

  const total = [...dimTotals.values()].reduce((s, v) => s + v, 0)
  const timeFmt = granularity === 'hour' ? 'MM-DD HH:mm' : granularity === 'month' ? 'YYYY-MM' : 'YYYY-MM-DD'

  const makeSpec = (type: 'area' | 'bar', values: DistRow[], stacked: boolean) => ({
    type,
    data: [{ id: `${type}Data`, values }],
    xField: 'Time',
    yField: 'Usage',
    seriesField: 'Model',
    stack: stacked,
    legends: { visible: true, selectMode: 'single' as const },
    color,
    tooltip: {
      mark: {
        content: [
          {
            key: (datum: Record<string, unknown>) => datum?.Model,
            value: (datum: Record<string, unknown>) => formatVal(Number(datum?.rawValue) || 0),
          },
        ],
      },
    },
    ...(type === 'area'
      ? {
          area: { style: { fillOpacity: 0.08, curveType: 'monotone' as const } },
          line: { style: { lineWidth: 2, curveType: 'monotone' as const } },
          point: { visible: false },
        }
      : {
          bar: { state: { hover: { stroke: '#000', lineWidth: 1 } } },
        }),
    background: 'transparent',
    animation: true,
    axes: [
      {
        orient: 'bottom',
        label: { formatMethod: (v: string) => dayjs(v).format(timeFmt) },
      },
    ],
  })

  return {
    spec_area: makeSpec('area', areaValues, false),
    spec_bar: makeSpec('bar', barValues, true),
    totalDisplay: formatVal(total),
    distKeys: [...new Set(areaValues.map((v) => v.Model))],
  }
}

// ---- model analytics chart specs ----

interface ModelSpecs {
  spec_trend: Record<string, unknown> | null
  spec_pie: Record<string, unknown> | null
  spec_rank: Record<string, unknown> | null
  totalDisplay: string
  modelNames: string[]
}

function buildModelSpecs(
  modelSeries: SeriesRow[],
  modelStats: StatsRow[],
  granularity: Granularity,
): ModelSpecs {
  const otherLabel = '其他'
  const formatInt = (n: number) => Intl.NumberFormat(undefined, { maximumFractionDigits: 0 }).format(n)

  if (!modelSeries.length || !modelStats.length) {
    return { spec_trend: null, spec_pie: null, spec_rank: null, totalDisplay: '0', modelNames: [] }
  }

  const modelCounts = new Map<string, number>()
  for (const s of modelStats) {
    const name = String(s.model || 'unknown')
    modelCounts.set(name, (modelCounts.get(name) || 0) + (Number(s.count) || 0))
  }

  const sortedModels = [...modelCounts.entries()].sort((a, b) => b[1] - a[1])
  const topModels = sortedModels.slice(0, TOP_N).map(([name]) => name)
  const otherModels = sortedModels.slice(TOP_N).map(([name]) => name)
  const totalCalls = sortedModels.reduce((s, [, c]) => s + c, 0)

  const timeMap = new Map<string, Map<string, number>>()
  const allTimes = new Set<string>()
  for (const row of modelSeries) {
    const time = String(row.time || '')
    const model = String(row.model || 'unknown')
    const count = Number(row.count) || 0
    allTimes.add(time)
    if (!timeMap.has(time)) timeMap.set(time, new Map())
    timeMap.get(time)!.set(model, (timeMap.get(time)!.get(model) || 0) + count)
  }

  let times = [...allTimes].sort()
  if (times.length > 0 && times.length < MAX_CHART_TREND_POINTS) {
    const last = dayjs(times[times.length - 1])
    const unit = granularity === 'hour' ? 'hour' : granularity === 'week' ? 'week' : granularity === 'month' ? 'month' : 'day'
    const fmt = granularity === 'hour' ? 'YYYY-MM-DD HH:00' : granularity === 'month' ? 'YYYY-MM' : 'YYYY-MM-DD'
    const padded: string[] = []
    for (let i = MAX_CHART_TREND_POINTS - 1; i >= 0; i--) {
      const t = last.subtract(i, unit).format(fmt)
      padded.push(t)
      if (!timeMap.has(t)) timeMap.set(t, new Map())
    }
    times = padded
  }

  type TrendRow = { Time: string; Model: string; Count: number }
  const trendValues: TrendRow[] = []
  for (const time of times) {
    const m = timeMap.get(time) || new Map()
    for (const model of topModels) {
      trendValues.push({ Time: time, Model: model, Count: m.get(model) || 0 })
    }
    if (otherModels.length > 0) {
      const otherCount = otherModels.reduce((s, model) => s + (m.get(model) || 0), 0)
      trendValues.push({ Time: time, Model: otherLabel, Count: otherCount })
    }
  }

  const pieValues = sortedModels.map(([type, value]) => ({ type, value }))

  let rankValues: { Model: string; Count: number }[]
  if (sortedModels.length > TOP_N) {
    const top = sortedModels.slice(0, TOP_N).map(([Model, Count]) => ({ Model, Count }))
    const otherCount = sortedModels.slice(TOP_N).reduce((s, [, c]) => s + c, 0)
    rankValues = [...top, { Model: otherLabel, Count: otherCount }]
  } else {
    rankValues = sortedModels.map(([Model, Count]) => ({ Model, Count }))
  }

  const colorDomain = [...topModels]
  if (otherModels.length > 0) colorDomain.push(otherLabel)
  const colorRange = MODEL_COLORS.slice(0, colorDomain.length)
  const color = { type: 'ordinal' as const, domain: colorDomain, range: colorRange }
  const timeFmt = granularity === 'hour' ? 'MM-DD HH:mm' : granularity === 'month' ? 'YYYY-MM' : 'YYYY-MM-DD'

  const spec_trend = {
    type: 'area' as const,
    data: [{ id: 'trendData', values: trendValues }],
    xField: 'Time',
    yField: 'Count',
    seriesField: 'Model',
    stack: false,
    legends: { visible: true, selectMode: 'single' as const },
    color,
    tooltip: {
      mark: {
        content: [
          {
            key: (datum: Record<string, unknown>) => datum?.Model,
            value: (datum: Record<string, unknown>) => formatInt(Number(datum?.Count) || 0),
          },
        ],
      },
    },
    area: { style: { fillOpacity: 0.08, curveType: 'monotone' as const } },
    line: { style: { lineWidth: 2, curveType: 'monotone' as const } },
    point: { visible: false },
    background: 'transparent',
    animation: true,
    axes: [
      { orient: 'bottom', label: { formatMethod: (v: string) => dayjs(v).format(timeFmt) } },
    ],
  }

  const spec_pie = {
    type: 'pie' as const,
    data: [{ id: 'pieData', values: pieValues }],
    outerRadius: 0.8,
    innerRadius: 0.5,
    padAngle: 0.6,
    valueField: 'value',
    categoryField: 'type',
    color,
    legends: { visible: true, orient: 'left' as const },
    label: { visible: true },
    tooltip: {
      mark: {
        content: [
          {
            key: (datum: Record<string, unknown>) => datum?.type,
            value: (datum: Record<string, unknown>) => formatInt(Number(datum?.value) || 0),
          },
        ],
      },
    },
    pie: {
      state: {
        hover: { outerRadius: 0.85, stroke: '#000', lineWidth: 1 },
        selected: { outerRadius: 0.85, stroke: '#000', lineWidth: 1 },
      },
    },
    background: 'transparent',
    animation: true,
  }

  const spec_rank = {
    type: 'bar' as const,
    data: [{ id: 'rankData', values: rankValues }],
    xField: 'Count',
    yField: 'Model',
    seriesField: 'Model',
    direction: 'horizontal' as const,
    color,
    legends: { visible: false },
    tooltip: {
      mark: {
        content: [
          {
            key: (datum: Record<string, unknown>) => datum?.Model,
            value: (datum: Record<string, unknown>) => formatInt(Number(datum?.Count) || 0),
          },
        ],
      },
    },
    bar: { state: { hover: { stroke: '#000', lineWidth: 1 } } },
    background: 'transparent',
    animation: true,
    axes: [
      { orient: 'left', type: 'band' },
      { orient: 'bottom', type: 'linear' },
    ],
  }

  return {
    spec_trend,
    spec_pie,
    spec_rank,
    totalDisplay: formatInt(totalCalls),
    modelNames: sortedModels.map(([name]) => name),
  }
}

// ---- component ----

const EMPTY_BOX = (height = 360) => (
  <div
    style={{
      display: 'grid',
      placeItems: 'center',
      height,
      color: 'var(--muted-foreground, #8a8680)',
      fontSize: 13,
    }}
  >
    当前时间范围内无数据
  </div>
)

export default function Dashboard() {
  const [data, setData] = useState<DashboardRangeData | null>(null)
  const [loading, setLoading] = useState(true)
  const [range, setRange] = useState<[Dayjs, Dayjs]>([dayjs().subtract(7, 'day'), dayjs()])
  const [granularity, setGranularity] = useState<Granularity>('day')
  const [distDimension, setDistDimension] = useState<DistDimension>('model')
  const [distMetric, setDistMetric] = useState<DistMetric>('token')
  const [distChartType, setDistChartType] = useState<DistChartType>('area')
  const [modelTab, setModelTab] = useState<ModelTab>('trend')
  const [themeReady, setThemeReady] = useState(false)
  const themeManagerRef = useRef<((typeof import('@visactor/vchart'))['ThemeManager']) | null>(null)

  useEffect(() => {
    setLoading(true)
    const start = range[0].format('YYYY-MM-DD HH:mm:ss')
    const end = range[1].format('YYYY-MM-DD HH:mm:ss')
    api
      .dashboardRange(start, end, granularity)
      .then((r) => setData(r.data))
      .finally(() => setLoading(false))
  }, [range, granularity])

  useEffect(() => {
    let cancelled = false
    const init = async () => {
      if (!themeManagerPromise) {
        themeManagerPromise = import('@visactor/vchart').then((m) => m.ThemeManager)
      }
      const ThemeManager = await themeManagerPromise
      if (!cancelled) {
        themeManagerRef.current = ThemeManager
        ThemeManager.setCurrentTheme('dark')
        setThemeReady(true)
      }
    }
    init()
    return () => { cancelled = true }
  }, [])

  // distribution specs
  const distSpecs = useMemo(() => {
    const series =
      distDimension === 'model'
        ? (data?.model_series || []) as SeriesRow[]
        : (data?.channel_series || []) as SeriesRow[]
    const stats =
      distDimension === 'model'
        ? (data?.model_stats || []) as StatsRow[]
        : (data?.distribution || []) as StatsRow[]
    const dimensionKey = distDimension === 'model' ? 'model' : 'channel_name'
    const metricKey = distMetric === 'token' ? 'total_tokens' : 'cost_rmb'
    return buildDistSpecs(series, stats, dimensionKey, metricKey, granularity)
  }, [data, distDimension, distMetric, granularity])

  // model analytics specs
  const modelSpecs = useMemo(
    () => buildModelSpecs((data?.model_series || []) as SeriesRow[], (data?.model_stats || []) as StatsRow[], granularity),
    [data, granularity],
  )

  const distSpec = distChartType === 'area' ? distSpecs.spec_area : distSpecs.spec_bar
  const distSpecType = typeof distSpec?.type === 'string' ? distSpec.type : distChartType

  const modelSpec =
    modelTab === 'trend'
      ? modelSpecs.spec_trend
      : modelTab === 'proportion'
        ? modelSpecs.spec_pie
        : modelSpecs.spec_rank
  const modelSpecType = typeof modelSpec?.type === 'string' ? modelSpec.type : modelTab

  const distChartKey = [
    distChartType,
    distSpecType,
    distDimension,
    distMetric,
    loading ? 'loading' : 'ready',
    data?.model_series?.length ?? 0,
  ].join('-')

  const modelChartKey = [
    modelTab,
    modelSpecType,
    loading ? 'loading' : 'ready',
    data?.model_series?.length ?? 0,
  ].join('-')

  const handleRangeChange = (dates: null | (Dayjs | null)[]) => {
    if (dates && dates[0] && dates[1]) {
      setRange([dates[0], dates[1]])
    }
  }

  if (loading && !data) {
    return (
      <div style={{ textAlign: 'center', padding: 80 }}>
        <Spin size="large" />
      </div>
    )
  }

  const distTotalLabel =
    distMetric === 'token'
      ? fmtNum(data?.total_tokens || 0) + ' tokens'
      : fmtRMB(data?.cost_rmb || 0)

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
            showTime={{ format: 'HH:mm' }}
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

      {/* 消耗分布 */}
      <Card style={{ marginTop: 16 }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12, flexWrap: 'wrap', gap: 8 }}>
          <div style={{ fontWeight: 500, fontSize: 15 }}>
            消耗分布
            <span style={{ marginLeft: 10, fontSize: 12, color: '#8a8680' }}>
              共 {distTotalLabel} · 按{distDimension === 'model' ? '模型' : '渠道'} {distSpecs.distKeys.length} 项
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
          {!distSpec || distSpecs.distKeys.length === 0 ? (
            EMPTY_BOX()
          ) : themeReady ? (
            <VChart
              key={distChartKey}
              spec={{ ...distSpec, theme: 'dark', background: 'transparent' } as ISpec}
              options={VCHART_OPTION}
            />
          ) : null}
        </div>
      </Card>

      {/* 模型调用分析 */}
      <Card style={{ marginTop: 16 }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12, flexWrap: 'wrap', gap: 8 }}>
          <div style={{ fontWeight: 500, fontSize: 15 }}>
            模型调用分析
            <span style={{ marginLeft: 10, fontSize: 12, color: '#8a8680' }}>
              共 {modelSpecs.totalDisplay} 次调用 · {modelSpecs.modelNames.length} 个模型
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
          {!modelSpec || modelSpecs.modelNames.length === 0 ? (
            EMPTY_BOX()
          ) : themeReady ? (
            <VChart
              key={modelChartKey}
              spec={{ ...modelSpec, theme: 'dark', background: 'transparent' } as ISpec}
              options={VCHART_OPTION}
            />
          ) : null}
        </div>
      </Card>
    </div>
  )
}
