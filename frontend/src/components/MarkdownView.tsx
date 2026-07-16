import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'

type Props = {
  content: string
  emptyText?: string
  className?: string
  maxHeight?: number | string
}

export default function MarkdownView({
  content,
  emptyText = '暂无内容',
  className = '',
  maxHeight = 360,
}: Props) {
  if (!content?.trim()) {
    return (
      <div className={`md-view md-view-empty ${className}`} style={{ maxHeight }}>
        {emptyText}
      </div>
    )
  }

  return (
    <div className={`md-view ${className}`} style={{ maxHeight }}>
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{content}</ReactMarkdown>
    </div>
  )
}
