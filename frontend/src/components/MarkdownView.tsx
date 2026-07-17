import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'

type Props = {
  content: string
  emptyText?: string
  className?: string
  maxHeight?: number | string
  bare?: boolean
}

const ROLE_LANGS = new Set([
  'USER',
  'ASSISTANT',
  'SYSTEM',
  'TOOL',
  'DEVELOPER',
  'FUNCTION',
  'UNKNOWN',
])

export function isRoleLang(lang: string): boolean {
  return ROLE_LANGS.has(lang.toUpperCase())
}

function nodeToText(node: any): string {
  if (node == null) return ''
  if (typeof node === 'string') return node
  if (typeof node === 'number') return String(node)
  if (Array.isArray(node)) return node.map(nodeToText).join('')
  if (typeof node === 'object' && node.props) return nodeToText(node.props.children)
  return ''
}

function PreBlock({ children, ...props }: any) {
  const child = Array.isArray(children) ? children[0] : children
  const className: string = child?.props?.className || ''
  const m = /language-([\w-]+)/.exec(className)
  const lang = m?.[1]
  if (!lang) {
    return <pre {...props}>{children}</pre>
  }
  const upper = lang.toUpperCase()
  if (ROLE_LANGS.has(upper)) {
    const text = nodeToText(child?.props?.children)
    return (
      <div className="md-code-block">
        <div className="md-code-block-lang md-code-block-lang-role">{upper}</div>
        <div className="md-code-block-body">
          <MarkdownView content={text} bare emptyText="*(empty)*" />
        </div>
      </div>
    )
  }
  return (
    <div className="md-code-block">
      <div className="md-code-block-lang">{upper}</div>
      <pre {...props}>{children}</pre>
    </div>
  )
}

export default function MarkdownView({
  content,
  emptyText = '暂无内容',
  className = '',
  maxHeight = 360,
  bare = false,
}: Props) {
  if (!content?.trim()) {
    if (bare) {
      return <div className={`md-view md-view-bare md-view-empty ${className}`}>{emptyText}</div>
    }
    return (
      <div className={`md-view md-view-empty ${className}`} style={{ maxHeight }}>
        {emptyText}
      </div>
    )
  }

  const cls = bare ? `md-view md-view-bare ${className}` : `md-view ${className}`
  return (
    <div className={cls} style={bare ? undefined : { maxHeight }}>
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={{ pre: PreBlock }}>
        {content}
      </ReactMarkdown>
    </div>
  )
}
