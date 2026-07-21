// 管理端登录态的唯一读写入口（token + username 始终成对增删）。

export function getToken(): string | null {
  return localStorage.getItem('token')
}

export function getUsername(): string {
  return localStorage.getItem('username') || 'admin'
}

export function setAuth(token: string, username: string): void {
  localStorage.setItem('token', token)
  localStorage.setItem('username', username)
}

export function clearAuth(): void {
  localStorage.removeItem('token')
  localStorage.removeItem('username')
}
