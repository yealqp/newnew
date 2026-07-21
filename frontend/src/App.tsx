import { Navigate, Outlet, Route, Routes } from 'react-router-dom'
import { ConfigProvider, theme, App as AntApp } from 'antd'
import zhCN from 'antd/locale/zh_CN'
import AppLayout from './layouts/AppLayout'
import Login from './pages/Login'
import Dashboard from './pages/Dashboard'
import Channels from './pages/Channels'
import Tokens from './pages/Tokens'
import Logs from './pages/Logs'
import Settings from './pages/Settings'
import Playground from './pages/Playground'
import { getToken } from './utils/auth'

function RequireAuth() {
  const token = getToken()
  if (!token) return <Navigate to="/login" replace />
  return <Outlet />
}

/** new-api Anthropic dark + Lora serif */
const SERIF =
  "'Lora', 'Source Serif 4', 'Noto Serif SC', 'Songti SC', Georgia, 'Times New Roman', serif"

export default function App() {
  return (
    <ConfigProvider
      locale={zhCN}
      theme={{
        algorithm: theme.darkAlgorithm,
        token: {
          colorPrimary: '#e08a6a',
          colorInfo: '#e08a6a',
          colorSuccess: '#8fba6e',
          colorWarning: '#d4a04a',
          colorError: '#e07070',
          colorBgBase: '#1c1b19',
          colorBgContainer: '#242320',
          colorBgElevated: '#2a2825',
          colorBgLayout: '#1c1b19',
          colorBorder: 'rgba(255,255,255,0.10)',
          colorBorderSecondary: 'rgba(255,255,255,0.06)',
          colorText: '#f5f3ee',
          colorTextSecondary: '#b8b4aa',
          colorTextTertiary: '#8a8680',
          colorLink: '#e08a6a',
          borderRadius: 10,
          borderRadiusLG: 12,
          borderRadiusSM: 6,
          fontFamily: SERIF,
          fontFamilyCode: "'JetBrains Mono', ui-monospace, monospace",
          controlHeight: 40,
          boxShadow: '0 1px 3px rgba(0,0,0,0.35)',
        },
        components: {
          Layout: {
            siderBg: '#1a1917',
            headerBg: '#1c1b19',
            bodyBg: '#1c1b19',
            triggerBg: '#33312d',
          },
          Menu: {
            darkItemBg: 'transparent',
            darkSubMenuItemBg: 'transparent',
            darkItemSelectedBg: '#35322e',
            darkItemSelectedColor: '#faf9f5',
            darkItemColor: '#b8b4aa',
            darkItemHoverColor: '#f5f3ee',
            itemBorderRadius: 8,
            fontFamily: SERIF,
          },
          Table: {
            headerBg: '#2e2c29',
            headerColor: '#b8b4aa',
            rowHoverBg: '#2e2c29',
            borderColor: 'rgba(255,255,255,0.10)',
            colorBgContainer: '#242320',
          },
          Card: {
            colorBgContainer: '#242320',
            borderRadiusLG: 10,
          },
          Button: {
            primaryShadow: 'none',
            borderRadius: 8,
            fontWeight: 500,
            fontFamily: SERIF,
          },
          Input: {
            colorBgContainer: '#2e2c29',
            activeBorderColor: '#e08a6a',
            hoverBorderColor: '#e08a6a',
            activeShadow: '0 0 0 3px rgba(224, 138, 106, 0.18)',
            fontFamily: SERIF,
          },
          Select: {
            colorBgContainer: '#2e2c29',
            optionSelectedBg: '#3a3732',
            fontFamily: SERIF,
          },
          Drawer: {
            colorBgElevated: '#242320',
          },
          Modal: {
            contentBg: '#2a2825',
            headerBg: '#2a2825',
            borderRadiusLG: 12,
          },
          Tag: {
            borderRadiusSM: 9999,
          },
          Typography: {
            fontFamily: SERIF,
            fontFamilyCode: "'JetBrains Mono', ui-monospace, monospace",
          },
        },
      }}
    >
      <AntApp>
        <Routes>
          <Route path="/login" element={<Login />} />
          <Route element={<RequireAuth />}>
            <Route element={<AppLayout />}>
              <Route index element={<Dashboard />} />
              <Route path="playground" element={<Playground />} />
              <Route path="channels" element={<Channels />} />
              <Route path="tokens" element={<Tokens />} />
              <Route path="logs" element={<Logs />} />
              <Route path="settings" element={<Settings />} />
            </Route>
          </Route>
          <Route path="*" element={<Navigate to="/" replace />} />
        </Routes>
      </AntApp>
    </ConfigProvider>
  )
}
