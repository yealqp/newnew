import { useMemo } from 'react'
import { Outlet, useLocation, useNavigate } from 'react-router-dom'
import { Layout, Menu, Button, Typography, Space } from 'antd'
import {
  LayoutDashboard,
  KeyRound,
  FileText,
  Settings,
  LogOut,
  Cloud,
  Sparkles,
} from 'lucide-react'
import { clearAuth, getUsername } from '../utils/auth'

const { Sider, Header, Content } = Layout

export default function AppLayout() {
  const nav = useNavigate()
  const loc = useLocation()

  const selected = useMemo(() => {
    const path = loc.pathname
    if (path.startsWith('/playground')) return ['/playground']
    if (path.startsWith('/channels')) return ['/channels']
    if (path.startsWith('/tokens')) return ['/tokens']
    if (path.startsWith('/logs')) return ['/logs']
    if (path.startsWith('/settings')) return ['/settings']
    return ['/']
  }, [loc.pathname])

  // Build menu items inside the component so icons render in a valid React tree.
  const items = useMemo(
    () => [
      { key: '/playground', icon: <Sparkles size={16} />, label: '游乐场' },
      { key: '/', icon: <LayoutDashboard size={16} />, label: '仪表盘' },
      { key: '/channels', icon: <Cloud size={16} />, label: '渠道' },
      { key: '/tokens', icon: <KeyRound size={16} />, label: '令牌' },
      { key: '/logs', icon: <FileText size={16} />, label: '日志' },
      { key: '/settings', icon: <Settings size={16} />, label: '设置' },
    ],
    [],
  )

  return (
    <Layout style={{ minHeight: '100vh' }}>
      <Sider width={232} breakpoint="lg" collapsedWidth={64}>
        <div
          style={{
            height: 64,
            display: 'flex',
            alignItems: 'center',
            gap: 12,
            padding: '0 20px',
            borderBottom: '1px solid #252320',
          }}
        >
          <div
            style={{
              width: 32,
              height: 32,
              borderRadius: 8,
              background: '#cc785c',
              display: 'grid',
              placeItems: 'center',
              color: '#fff',
              flexShrink: 0,
              fontFamily: "'Cormorant Garamond', serif",
              fontSize: 18,
              fontWeight: 500,
            }}
          >
            ✦
          </div>
          <div style={{ overflow: 'hidden' }}>
            <div
              style={{
                fontFamily: "'Cormorant Garamond', serif",
                fontWeight: 500,
                fontSize: 20,
                lineHeight: 1.15,
                letterSpacing: '-0.3px',
                color: '#faf9f5',
              }}
            >
              OpenGate
            </div>
            <div style={{ fontSize: 11, color: '#a09d96', marginTop: 2 }}>AI 聚合网关</div>
          </div>
        </div>
        <Menu
          theme="dark"
          mode="inline"
          selectedKeys={selected}
          items={items}
          onClick={({ key }) => nav(key)}
          style={{ marginTop: 12, border: 'none', background: 'transparent' }}
        />
      </Sider>
      <Layout>
        <Header
          style={{
            background: '#1c1b19',
            borderBottom: '1px solid rgba(255,255,255,0.10)',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            padding: '0 24px',
            height: 64,
          }}
        >
          <Typography.Text style={{ color: '#8a8680', fontSize: 13, fontWeight: 500 }}>
            用量统计 · 渠道转发
          </Typography.Text>
          <Space size="middle">
            <Typography.Text style={{ fontSize: 13, color: '#e8e4dc', fontWeight: 500 }}>
              {getUsername()}
            </Typography.Text>
            <Button
              type="text"
              icon={<LogOut size={16} />}
              style={{ color: '#8a8680' }}
              onClick={() => {
                clearAuth()
                nav('/login')
              }}
            >
              退出
            </Button>
          </Space>
        </Header>
        <Content style={{ padding: 24, overflow: 'auto', background: '#1c1b19' }}>
          <div style={{ width: '100%', minWidth: 0 }}>
            <Outlet />
          </div>
        </Content>
      </Layout>
    </Layout>
  )
}
