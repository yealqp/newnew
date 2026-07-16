/**
 * Extract human-readable markdown from stored request/response bodies.
 * Supports OpenAI/Claude JSON (non-stream) and SSE stream dumps.
 */

export type ExtractedContent = {
  markdown: string
  format: 'openai' | 'claude' | 'stream-openai' | 'stream-claude' | 'text' | 'empty'
  hasContent: boolean
}

export function extractLogContent(raw: string | undefined | null, kind: 'request' | 'response'): ExtractedContent {
  if (!raw || !raw.trim()) {
    return { markdown: '', format: 'empty', hasContent: false }
  }
  const s = raw.trim()

  // SSE stream dump (OpenAI / Claude)
  if (looksLikeSSE(s)) {
    const stream = parseSSE(s)
    if (stream.markdown) {
      return {
        markdown: stream.markdown,
        format: stream.format,
        hasContent: true,
      }
    }
  }

  // JSON body
  try {
    const obj = JSON.parse(s)
    if (kind === 'request') {
      return extractRequestJSON(obj)
    }
    return extractResponseJSON(obj)
  } catch {
    // plain text / truncated junk
    return { markdown: s, format: 'text', hasContent: true }
  }
}

function looksLikeSSE(s: string): boolean {
  return (
    s.includes('data:') &&
    (s.includes('"choices"') ||
      s.includes('content_block_delta') ||
      s.includes('message_start') ||
      s.includes('chat.completion.chunk') ||
      s.includes('normalizedUsage') ||
      s.includes('x-opencode-type') ||
      s.includes('[DONE]'))
  )
}

function parseSSE(s: string): { markdown: string; format: 'stream-openai' | 'stream-claude' } {
  let isClaude = s.includes('content_block_delta') || s.includes('message_start') || s.includes('event:')
  let text = ''
  const toolParts: string[] = []
  const metaParts: string[] = []
  let role = ''
  let usageLine = ''

  const lines = s.split(/\r?\n/)
  for (const line of lines) {
    const trimmed = line.trim()
    if (!trimmed.startsWith('data:')) continue
    const data = trimmed.slice(5).trim()
    if (!data || data === '[DONE]') continue

    let obj: any
    try {
      obj = JSON.parse(data)
    } catch {
      continue
    }

    // OpenCode-GO trailing cost / usage-only chunk (empty choices)
    if (obj['x-opencode-type'] === 'inference-cost' || obj.normalizedUsage) {
      const nu = obj.normalizedUsage || {}
      const usage = obj.usage || {}
      const pt = usage.prompt_tokens ?? nu.inputTokens
      const ct = usage.completion_tokens ?? nu.outputTokens
      const cr = usage.prompt_tokens_details?.cached_tokens ?? nu.cacheReadTokens
      if (pt != null || ct != null) {
        usageLine = `> usage: prompt \`${pt ?? '-'}\` · completion \`${ct ?? '-'}\`${
          cr != null ? ` · cache_read \`${cr}\`` : ''
        }`
      }
      if (obj.cost != null) {
        metaParts.push(`cost: \`${obj.cost}\``)
      }
      continue
    }

    // OpenAI chunk
    if (obj.object === 'chat.completion.chunk' || Array.isArray(obj.choices)) {
      isClaude = false
      if (obj.usage) {
        const u = obj.usage
        usageLine = `> usage: prompt \`${u.prompt_tokens ?? '-'}\` · completion \`${u.completion_tokens ?? '-'}\``
      }
      const choice = obj.choices?.[0]
      if (!choice) continue
      const delta = choice.delta || {}
      if (delta.role && !role) role = delta.role
      if (typeof delta.content === 'string') text += delta.content
      if (Array.isArray(delta.tool_calls)) {
        for (const tc of delta.tool_calls) {
          const name = tc.function?.name || ''
          const args = tc.function?.arguments || ''
          if (name) toolParts.push(`**tool_call** \`${name}\``)
          if (args) toolParts.push('```json\n' + args + '\n```')
        }
      }
      continue
    }

    // Claude events
    const typ = obj.type || ''
    if (typ === 'content_block_delta') {
      isClaude = true
      const d = obj.delta || {}
      if (d.type === 'text_delta' && typeof d.text === 'string') text += d.text
      if (d.type === 'input_json_delta' && typeof d.partial_json === 'string') {
        toolParts.push(d.partial_json)
      }
    } else if (typ === 'content_block_start') {
      isClaude = true
      const cb = obj.content_block || {}
      if (cb.type === 'tool_use') {
        toolParts.push(`**tool_use** \`${cb.name || ''}\` (\`${cb.id || ''}\`)`)
      }
      if (cb.type === 'text' && typeof cb.text === 'string') text += cb.text
    } else if (typ === 'message_delta' && obj.usage) {
      isClaude = true
      const u = obj.usage
      usageLine = `> usage: input \`${u.input_tokens ?? '-'}\` · output \`${u.output_tokens ?? '-'}\``
    }
  }

  let markdown = text.trim()
  if (toolParts.length) {
    const tools = toolParts.join('\n\n')
    markdown = markdown ? `${markdown}\n\n---\n\n${tools}` : tools
  }
  if (usageLine) {
    markdown = markdown ? `${usageLine}\n\n${markdown}` : usageLine
  }
  if (metaParts.length) {
    markdown = markdown
      ? `${markdown}\n\n${metaParts.join(' · ')}`
      : metaParts.join(' · ')
  }
  return {
    markdown,
    format: isClaude ? 'stream-claude' : 'stream-openai',
  }
}

function extractRequestJSON(obj: any): ExtractedContent {
  // OpenAI chat: messages
  if (Array.isArray(obj.messages)) {
    const parts: string[] = []
    if (obj.model) parts.push(`> model: \`${obj.model}\`${obj.stream ? ' · stream' : ''}`)
    for (const m of obj.messages) {
      const role = m.role || 'unknown'
      const body = contentToMarkdown(m.content)
      const extra: string[] = []
      if (Array.isArray(m.tool_calls) && m.tool_calls.length) {
        for (const tc of m.tool_calls) {
          const name = tc.function?.name || tc.name || ''
          const args = tc.function?.arguments || tc.arguments || ''
          extra.push(`**tool_call** \`${name}\`\n\`\`\`json\n${tryPretty(args)}\n\`\`\``)
        }
      }
      if (m.tool_call_id) {
        extra.push(`tool_call_id: \`${m.tool_call_id}\``)
      }
      parts.push(`### ${role}\n\n${body || '*(empty)*'}${extra.length ? '\n\n' + extra.join('\n\n') : ''}`)
    }
    return { markdown: parts.join('\n\n'), format: 'openai', hasContent: true }
  }

  // Claude: system + messages
  if (Array.isArray(obj.messages) || obj.system !== undefined) {
    const parts: string[] = []
    if (obj.model) parts.push(`> model: \`${obj.model}\`${obj.stream ? ' · stream' : ''}`)
    const sys = contentToMarkdown(obj.system)
    if (sys) parts.push(`### system\n\n${sys}`)
    if (Array.isArray(obj.messages)) {
      for (const m of obj.messages) {
        const role = m.role || 'unknown'
        parts.push(`### ${role}\n\n${contentToMarkdown(m.content) || '*(empty)*'}`)
      }
    }
    if (parts.length) {
      return { markdown: parts.join('\n\n'), format: 'claude', hasContent: true }
    }
  }

  return {
    markdown: '```json\n' + JSON.stringify(obj, null, 2) + '\n```',
    format: 'text',
    hasContent: true,
  }
}

function extractResponseJSON(obj: any): ExtractedContent {
  // OpenAI chat completion
  if (Array.isArray(obj.choices)) {
    const parts: string[] = []
    if (obj.model) parts.push(`> model: \`${obj.model}\``)
    for (let i = 0; i < obj.choices.length; i++) {
      const c = obj.choices[i]
      const msg = c.message || c.delta || {}
      const role = msg.role || 'assistant'
      const body = contentToMarkdown(msg.content)
      const extras: string[] = []
      if (Array.isArray(msg.tool_calls)) {
        for (const tc of msg.tool_calls) {
          const name = tc.function?.name || ''
          const args = tc.function?.arguments || ''
          extras.push(`**tool_call** \`${name}\`\n\`\`\`json\n${tryPretty(args)}\n\`\`\``)
        }
      }
      if (c.finish_reason) extras.push(`finish_reason: \`${c.finish_reason}\``)
      const title = obj.choices.length > 1 ? `### ${role} #${i}` : `### ${role}`
      parts.push(`${title}\n\n${body || '*(empty)*'}${extras.length ? '\n\n' + extras.join('\n\n') : ''}`)
    }
    return { markdown: parts.join('\n\n'), format: 'openai', hasContent: true }
  }

  // Claude message response
  if (obj.type === 'message' || Array.isArray(obj.content)) {
    const parts: string[] = []
    if (obj.model) parts.push(`> model: \`${obj.model}\``)
    if (obj.role) parts.push(`### ${obj.role}`)
    const body = contentToMarkdown(obj.content)
    parts.push(body || '*(empty)*')
    if (obj.stop_reason) parts.push(`\nstop_reason: \`${obj.stop_reason}\``)
    return { markdown: parts.join('\n\n'), format: 'claude', hasContent: true }
  }

  // error shapes
  if (obj.error) {
    const msg =
      typeof obj.error === 'string'
        ? obj.error
        : obj.error.message || JSON.stringify(obj.error, null, 2)
    return {
      markdown: `**Error**\n\n\`\`\`\n${msg}\n\`\`\``,
      format: 'text',
      hasContent: true,
    }
  }

  return {
    markdown: '```json\n' + JSON.stringify(obj, null, 2) + '\n```',
    format: 'text',
    hasContent: true,
  }
}

function contentToMarkdown(content: unknown): string {
  if (content == null) return ''
  if (typeof content === 'string') return content
  if (Array.isArray(content)) {
    const chunks: string[] = []
    for (const part of content) {
      if (typeof part === 'string') {
        chunks.push(part)
        continue
      }
      if (!part || typeof part !== 'object') continue
      const p = part as Record<string, any>
      const type = p.type
      if (type === 'text' || typeof p.text === 'string') {
        if (p.text) chunks.push(String(p.text))
      } else if (type === 'image_url') {
        const url = p.image_url?.url || p.image_url || ''
        chunks.push(url ? `![image](${url})` : '*[image]*')
      } else if (type === 'image') {
        chunks.push('*[image]*')
      } else if (type === 'tool_use') {
        chunks.push(
          `**tool_use** \`${p.name || ''}\` (\`${p.id || ''}\`)\n\`\`\`json\n${JSON.stringify(p.input ?? {}, null, 2)}\n\`\`\``,
        )
      } else if (type === 'tool_result') {
        const c = contentToMarkdown(p.content)
        chunks.push(`**tool_result** (\`${p.tool_use_id || ''}\`)\n\n${c}`)
      } else if (type === 'input_json') {
        chunks.push('```json\n' + JSON.stringify(p, null, 2) + '\n```')
      } else {
        chunks.push('```json\n' + JSON.stringify(p, null, 2) + '\n```')
      }
    }
    return chunks.join('\n\n')
  }
  if (typeof content === 'object') {
    return '```json\n' + JSON.stringify(content, null, 2) + '\n```'
  }
  return String(content)
}

function tryPretty(s: unknown): string {
  if (typeof s !== 'string') return JSON.stringify(s, null, 2)
  try {
    return JSON.stringify(JSON.parse(s), null, 2)
  } catch {
    return s
  }
}

export function prettyRaw(s: string): string {
  if (!s) return '-'
  try {
    return JSON.stringify(JSON.parse(s), null, 2)
  } catch {
    return s
  }
}
