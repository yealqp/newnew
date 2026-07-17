import axios from 'axios'
import { message } from 'antd'

const client = axios.create({
  baseURL: '/api/admin',
  timeout: 30000,
})

client.interceptors.request.use((config) => {
  const url = config.url || ''
  // login must not carry a stale JWT
  if (url.includes('/login')) {
    if (config.headers) {
      delete (config.headers as Record<string, unknown>).Authorization
    }
    return config
  }
  const token = localStorage.getItem('token')
  if (token) {
    config.headers.Authorization = `Bearer ${token}`
  }
  return config
})

client.interceptors.response.use(
  (res) => {
    const data = res.data
    if (data && data.success === false) {
      message.error(data.message || '请求失败')
      return Promise.reject(new Error(data.message || '请求失败'))
    }
    return data?.data !== undefined ? { ...res, data: data.data } : res
  },
  (err) => {
    const status = err.response?.status
    const body = err.response?.data
    const msg =
      body?.message ||
      (typeof body?.error === 'string' ? body.error : body?.error?.message) ||
      err.message ||
      '网络错误'
    const onLoginPage =
      typeof window !== 'undefined' && window.location.pathname.includes('/login')
    const isLoginRequest = (err.config?.url || '').includes('/login')

    if (status === 401 && !isLoginRequest) {
      localStorage.removeItem('token')
      if (!onLoginPage) {
        window.location.href = '/login'
      }
    }
    message.error(msg)
    return Promise.reject(err)
  },
)

export default client

// ---- types ----

export interface Channel {
  id: number
  icon: string
  name: string
  type: 'openai' | 'claude'
  base_url: string
  full_url: boolean
  api_key: string
  models: string
  model_mapping: string
  status: number
  weight: number
  priority: number
  pricing: string
  remark: string
  response_time?: number
  test_time?: number
  total_tokens?: number
  prompt_tokens?: number
  completion_tokens?: number
  requests?: number
  cost_rmb?: number
  created_at: string
  updated_at: string
}

export interface Token {
  id: number
  name: string
  key: string
  status: number
  model_limits: string
  expired_at: number
  created_at: string
  accessed_at?: string
}

export interface RequestLog {
  id: number
  created_at: string
  request_id: string
  token_id: number
  token_name: string
  channel_id: number
  channel_name: string
  model: string
  upstream_model: string
  is_stream: boolean
  first_token_ms: number
  duration_ms: number
  prompt_tokens: number
  completion_tokens: number
  cache_read_tokens: number
  cache_write_tokens: number
  total_tokens: number
  cost_rmb: number
  status: string
  error_message: string
  ip: string
  request_body: string
  response_body: string
  detail: string
}

export interface DashboardData {
  today: {
    requests: number
    prompt_tokens: number
    completion_tokens: number
    total_tokens: number
    cost_rmb: number
  }
  total: {
    requests: number
    prompt_tokens: number
    completion_tokens: number
    total_tokens: number
    cost_rmb: number
  }
  series: Array<{
    day: string
    requests: number
    prompt_tokens: number
    completion_tokens: number
    cost_rmb: number
  }>
  channel_count: number
  token_count: number
}

export interface DashboardRangeData {
  requests: number
  prompt_tokens: number
  completion_tokens: number
  total_tokens: number
  cost_rmb: number
  rpm: number
  tpm: number
  series: Array<{
    time: string
    requests: number
    prompt_tokens: number
    completion_tokens: number
    total_tokens: number
    cost_rmb: number
  }>
  distribution: Array<{
    channel_name: string
    prompt_tokens: number
    completion_tokens: number
    total_tokens: number
    cost_rmb: number
  }>
  model_stats: Array<{
    model: string
    count: number
    prompt_tokens: number
    completion_tokens: number
    total_tokens: number
    cost_rmb: number
  }>
  model_series: Array<{
    time: string
    model: string
    count: number
    prompt_tokens: number
    completion_tokens: number
    total_tokens: number
    cost_rmb: number
  }>
  channel_series: Array<{
    time: string
    channel_name: string
    prompt_tokens: number
    completion_tokens: number
    total_tokens: number
    cost_rmb: number
  }>
  start?: string
  end?: string
}

export interface ModelPrice {
  input: number
  output: number
  cache_read: number
  cache_write: number
}

// ---- api ----

export const api = {
  login: (username: string, password: string) =>
    client.post<{ token: string; username: string }>('/login', { username, password }),

  me: () => client.get('/me'),

  updateAccount: (data: { old_password: string; new_username?: string; new_password?: string }) =>
    client.post<{ username: string; token: string }>('/change-password', data),

  listChannels: () => client.get<Channel[]>('/channels'),
  getChannel: (id: number) => client.get<Channel>(`/channels/${id}`),
  createChannel: (data: Partial<Channel>) => client.post<Channel>('/channels', data),
  updateChannel: (id: number, data: Partial<Channel>) => client.put<Channel>(`/channels/${id}`, data),
  deleteChannel: (id: number) => client.delete(`/channels/${id}`),
  testChannel: (id: number, model?: string) =>
    client.get<{
      channel_id: number
      model: string
      upstream_model: string
      response_time: number
      time: number
      status_code: number
      preview?: string
    }>(`/channels/${id}/test`, { params: model ? { model } : undefined }),
  fetchUpstreamModels: (data: {
    base_url: string
    api_key?: string
    type: string
    full_url?: boolean
    channel_id?: number
  }) => client.post<{ models: string[]; count: number }>('/channels/fetch-models', data),

  listTokens: () => client.get<Token[]>('/tokens'),
  createToken: (data: Partial<Token>) => client.post<Token>('/tokens', data),
  updateToken: (id: number, data: Partial<Token>) => client.put<Token>(`/tokens/${id}`, data),
  resetTokenKey: (id: number) => client.post<Token>(`/tokens/${id}/reset-key`),
  deleteToken: (id: number) => client.delete(`/tokens/${id}`),

  listLogs: (params: Record<string, string | number>) =>
    client.get<{ list: RequestLog[]; total: number; page: number; page_size: number }>('/logs', { params }),
  getLog: (id: number) => client.get<RequestLog>(`/logs/${id}`),

  dashboard: () => client.get<DashboardData>('/dashboard'),
  dashboardRange: (start: string, end: string, granularity?: string) =>
    client.get<DashboardRangeData>('/dashboard', { params: { start, end, granularity } }),

  getSettings: () => client.get<Record<string, string>>('/settings'),
  updateSettings: (data: Record<string, string>) => client.put('/settings', data),
}
