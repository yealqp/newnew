import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { Button, Form, Input, message } from 'antd'
import { Lock, User, Sparkles } from 'lucide-react'
import { api } from '../api/client'
import { GateLogo } from '../components/GateLogo'

export default function Setup() {
  const nav = useNavigate()
  const [loading, setLoading] = useState(false)
  const [checking, setChecking] = useState(true)

  useEffect(() => {
    api
      .setupStatus()
      .then((res: any) => {
        if (res.data?.initialized) {
          nav('/login', { replace: true })
        }
      })
      .catch(() => { /* ignore */ })
      .finally(() => setChecking(false))
  }, [nav])

  const onFinish = async (values: { username: string; password: string }) => {
    setLoading(true)
    try {
      const res = await api.setup(values.username, values.password)
      const token = res.data?.token
      if (!token) {
        message.error('初始化失败：未返回 token')
        return
      }
      localStorage.setItem('token', token)
      localStorage.setItem('username', res.data.username || values.username)
      message.success('初始化成功')
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
            <p>首次使用 · 创建管理员账户</p>
          </div>
        </div>
        <Form layout="vertical" onFinish={onFinish} size="large">
          <Form.Item
            name="username"
            rules={[
              { required: true, message: '请输入用户名' },
              { min: 3, message: '用户名至少 3 个字符' },
            ]}
          >
            <Input prefix={<User size={16} />} placeholder="用户名" autoComplete="username" />
          </Form.Item>
          <Form.Item
            name="password"
            rules={[
              { required: true, message: '请输入密码' },
              { min: 6, message: '密码至少 6 个字符' },
            ]}
          >
            <Input.Password
              prefix={<Lock size={16} />}
              placeholder="密码"
              autoComplete="new-password"
            />
          </Form.Item>
          <Form.Item
            name="confirm"
            dependencies={['password']}
            rules={[
              { required: true, message: '请确认密码' },
              ({ getFieldValue }) => ({
                validator(_, value) {
                  if (!value || getFieldValue('password') === value) return Promise.resolve()
                  return Promise.reject(new Error('两次输入的密码不一致'))
                },
              }),
            ]}
          >
            <Input.Password
              prefix={<Lock size={16} />}
              placeholder="确认密码"
              autoComplete="new-password"
            />
          </Form.Item>
          <Button
            type="primary"
            htmlType="submit"
            block
            loading={loading}
            icon={<Sparkles size={16} />}
            style={{ height: 44, marginTop: 8 }}
          >
            初始化系统
          </Button>
        </Form>
      </div>
    </div>
  )
}
