import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { Button, Form, Input, message } from 'antd'
import { Lock, User } from 'lucide-react'
import { api } from '../api/client'
import { clearAuth, setAuth } from '../utils/auth'
import { GateLogo } from '../components/GateLogo'

export default function Login() {
  const nav = useNavigate()
  const [loading, setLoading] = useState(false)
  const [checking, setChecking] = useState(true)

  useEffect(() => {
    api
      .setupStatus()
      .then((res: any) => {
        if (!res.data?.initialized) {
          nav('/setup', { replace: true })
        }
      })
      .catch(() => { /* ignore */ })
      .finally(() => setChecking(false))
  }, [nav])

  const onFinish = async (values: { username: string; password: string }) => {
    setLoading(true)
    try {
      clearAuth()
      const res = await api.login(values.username, values.password)
      const token = res.data?.token
      if (!token) {
        message.error('登录响应异常：未返回 token')
        return
      }
      setAuth(token, res.data.username || values.username)
      message.success('登录成功')
      nav('/', { replace: true })
    } catch {
      // toast by interceptor
    } finally {
      setLoading(false)
    }
  }

  if (checking) return null

  return (
    <div className="login-bg">
      <div className="login-card">
        <div className="login-brand">
          <div className="logo"><GateLogo size={40} /></div>
          <div>
            <h1>OpenGate</h1>
            <p>OpenAI / Claude 聚合网关</p>
          </div>
        </div>
        <Form layout="vertical" onFinish={onFinish} size="large">
          <Form.Item name="username" rules={[{ required: true, message: '请输入用户名' }]}>
            <Input prefix={<User size={16} />} placeholder="用户名" autoComplete="username" />
          </Form.Item>
          <Form.Item name="password" rules={[{ required: true, message: '请输入密码' }]}>
            <Input.Password prefix={<Lock size={16} />} placeholder="密码" autoComplete="current-password" />
          </Form.Item>
          <Button type="primary" htmlType="submit" block loading={loading} style={{ height: 44, marginTop: 8 }}>
            登录
          </Button>
        </Form>
      </div>
    </div>
  )
}
