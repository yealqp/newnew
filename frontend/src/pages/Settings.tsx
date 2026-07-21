import { useEffect, useState } from 'react'
import { Button, Card, Form, Input, InputNumber, Select, message } from 'antd'
import { Save, Key } from 'lucide-react'
import { api } from '../api/client'
import { getUsername, setAuth } from '../utils/auth'

export default function Settings() {
  const [form] = Form.useForm()
  const [pwdForm] = Form.useForm()
  const [loading, setLoading] = useState(false)

  useEffect(() => {
    api.getSettings().then((r) => {
      form.setFieldsValue({
        log_body_max_bytes: Number(r.data.log_body_max_bytes || 65536),
        price_missing_policy: r.data.price_missing_policy || 'allow',
        request_timeout: Number(r.data.request_timeout || 300),
      })
    })
  }, [form])

  const saveSettings = async () => {
    const v = await form.validateFields()
    setLoading(true)
    try {
      await api.updateSettings({
        log_body_max_bytes: String(v.log_body_max_bytes),
        price_missing_policy: v.price_missing_policy,
        request_timeout: String(v.request_timeout),
      })
      message.success('设置已保存')
    } finally {
      setLoading(false)
    }
  }

  const currentUsername = getUsername()

  useEffect(() => {
    pwdForm.setFieldsValue({ new_username: currentUsername })
  }, [pwdForm, currentUsername])

  const changeAccount = async () => {
    const v = await pwdForm.validateFields()
    const newUsername = (v.new_username || '').trim()
    const usernameChanged = newUsername && newUsername !== currentUsername
    if (!usernameChanged && !v.new_password) {
      message.warning('未做任何修改')
      return
    }
    const r = await api.updateAccount({
      old_password: v.old_password,
      new_username: usernameChanged ? newUsername : undefined,
      new_password: v.new_password || undefined,
    })
    if (r.data?.token && r.data?.username) setAuth(r.data.token, r.data.username)
    message.success('账户已更新')
    pwdForm.resetFields()
    pwdForm.setFieldsValue({ new_username: r.data?.username || newUsername || currentUsername })
  }

  return (
    <div>
      <h1 className="page-title">设置</h1>
      <p className="page-desc">系统参数与管理员账户</p>

      <Card title="运行参数" style={{ marginBottom: 16 }}>
        <Form form={form} layout="vertical">
          <Form.Item
            name="request_timeout"
            label="上游请求超时（秒）"
            rules={[{ required: true }]}
          >
            <InputNumber min={10} max={3600} style={{ width: '100%' }} />
          </Form.Item>
          <Form.Item
            name="log_body_max_bytes"
            label="日志 Body 最大字节"
            rules={[{ required: true }]}
          >
            <InputNumber min={1024} max={10_000_000} style={{ width: '100%' }} />
          </Form.Item>
          <Form.Item
            name="price_missing_policy"
            label="模型未配置价格时"
            extra="allow：仍转发并标记 price_missing；reject：直接拒绝"
          >
            <Select
              options={[
                { value: 'allow', label: '允许转发 (allow)' },
                { value: 'reject', label: '拒绝请求 (reject)' },
              ]}
            />
          </Form.Item>
          <Button type="primary" loading={loading} onClick={saveSettings} icon={<Save size={16} />}>
            保存设置
          </Button>
        </Form>
      </Card>

      <Card title="修改账户">
        <Form form={pwdForm} layout="vertical">
          <Form.Item
            name="new_username"
            label="用户名"
            rules={[{ min: 3, message: '至少 3 位' }]}
          >
            <Input autoComplete="username" placeholder="用户名" />
          </Form.Item>
          <Form.Item
            name="old_password"
            label="当前密码"
            rules={[{ required: true, message: '请输入当前密码以确认修改' }]}
          >
            <Input.Password autoComplete="current-password" />
          </Form.Item>
          <Form.Item
            name="new_password"
            label="新密码"
            extra="留空表示不修改密码"
            rules={[{ min: 6, message: '至少 6 位' }]}
          >
            <Input.Password autoComplete="new-password" />
          </Form.Item>
          <Form.Item
            name="confirm"
            label="确认新密码"
            dependencies={['new_password']}
            rules={[
              ({ getFieldValue }) => ({
                validator(_, value) {
                  const np = getFieldValue('new_password')
                  if (!np) return Promise.resolve()
                  if (value === np) return Promise.resolve()
                  return Promise.reject(new Error('两次输入不一致'))
                },
              }),
            ]}
          >
            <Input.Password autoComplete="new-password" />
          </Form.Item>
          <Button type="primary" onClick={changeAccount} icon={<Key size={16} />}>
            保存账户
          </Button>
        </Form>
      </Card>
    </div>
  )
}
