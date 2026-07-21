/**
 * Extract human-readable markdown from stored request/response bodies.
 * Supports OpenAI/Claude JSON (non-stream) and SSE stream dumps.
 */

export type ExtractedContent = {
  markdown: string
  format: 'openai' | 'claude' | 'stream-openai' | 'stream-claude' | 'text' | 'empty'
  hasContent: boolean
}

/**
 * Iterate SSE `data:` payloads from a raw stream dump, skipping blank
 * payloads and the terminal "[DONE]" sentinel.
 */
export function* iterateSSEData(text: string): Generator<string> {
  const lines = text.split(/\r?\n/)
  for (const line of lines) {
    const trimmed = line.trim()
    if (!trimmed.startsWith('data:')) continue
    const data = trimmed.slice(5).trim()
    if (!data || data === '[DONE]') continue
    yield data
  }
}

/** Pretty-print a JSON string, or return null if it isn't valid JSON. */
function tryPrettyJSON(s: string): string | null {
  try {
    return JSON.stringify(JSON.parse(s), null, 2)
  } catch {
    return null
  }
}

function roleLabel(role: unknown): string {
  return String(role || 'unknown').toUpperCase()
}

function toolCallsToMarkdown(toolCalls: any[]): string[] {
  const parts: string[] = []
  for (const tc of toolCalls) {
    const name = tc.function?.name || tc.name || ''
    const args = tc.function?.arguments || tc.arguments || ''
    parts.push(`**tool_call** \`${name}\`\n\`\`\`json\n${tryPretty(args)}\n\`\`\``)
  }
  return parts
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

  for (const data of iterateSSEData(s)) {
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
      const role = roleLabel(m.role)
      const body = contentToMarkdown(m.content)
      const extra: string[] = []
      if (Array.isArray(m.tool_calls) && m.tool_calls.length) {
        extra.push(...toolCallsToMarkdown(m.tool_calls))
      }
      if (m.tool_call_id) {
        extra.push(`tool_call_id: \`${m.tool_call_id}\``)
      }
      const content = body || '*(empty)*'
      const extras = extra.length ? '\n\n' + extra.join('\n\n') : ''
      parts.push(`\`\`\`${role}\n${content}\n\`\`\`${extras}`)
    }
    return { markdown: parts.join('\n\n'), format: 'openai', hasContent: true }
  }

  // Claude: system + messages
  if (Array.isArray(obj.messages) || obj.system !== undefined) {
    const parts: string[] = []
    if (obj.model) parts.push(`> model: \`${obj.model}\`${obj.stream ? ' · stream' : ''}`)
    const sys = contentToMarkdown(obj.system)
    if (sys) parts.push(`\`\`\`SYSTEM\n${sys}\n\`\`\``)
    if (Array.isArray(obj.messages)) {
      for (const m of obj.messages) {
        const role = roleLabel(m.role)
        const body = contentToMarkdown(m.content) || '*(empty)*'
        parts.push(`\`\`\`${role}\n${body}\n\`\`\``)
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
      const reasoning = contentToMarkdown(msg.reasoning_content)
      const extras: string[] = []
      if (Array.isArray(msg.tool_calls)) {
        extras.push(...toolCallsToMarkdown(msg.tool_calls))
      }
      if (reasoning) {
        extras.unshift(`#### 思考过程\n\n${reasoning}`)
      }
      if (c.finish_reason) extras.push(`finish_reason: \`${c.finish_reason}\``)
      const usage = formatOpenAIUsage(obj.usage)
      if (usage && i === 0) extras.push(usage)
      const title = obj.choices.length > 1 ? `### ${role} #${i}` : `### ${role}`
      const visible = body || (reasoning ? '*（无最终回复内容）*' : '*(empty)*')
      parts.push(`${title}\n\n${visible}${extras.length ? '\n\n' + extras.join('\n\n') : ''}`)
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

function formatOpenAIUsage(usage: any): string {
  if (!usage) return ''
  const prompt = usage.prompt_tokens
  const completion = usage.completion_tokens
  const total = usage.total_tokens ?? ((prompt || 0) + (completion || 0))
  const cached = usage.prompt_tokens_details?.cached_tokens
  const reasoning = usage.completion_tokens_details?.reasoning_tokens
  const text = usage.completion_tokens_details?.text_tokens
  const segments = [
    prompt != null ? `输入 \`${Number(prompt).toLocaleString()}\`` : '',
    completion != null ? `输出 \`${Number(completion).toLocaleString()}\`` : '',
    total != null ? `总计 \`${Number(total).toLocaleString()}\`` : '',
    cached != null ? `缓存读取 \`${Number(cached).toLocaleString()}\`` : '',
    reasoning != null ? `推理 \`${Number(reasoning).toLocaleString()}\`` : '',
    text != null ? `文本 \`${Number(text).toLocaleString()}\`` : '',
  ].filter(Boolean)
  return segments.length ? `> Token：${segments.join(' · ')}` : ''
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
  return tryPrettyJSON(s) ?? s
}

/**
 * Pretty-print JSON from a stored body, handling both single JSON objects
 * and SSE streams with multiple data: JSON payloads.
 */
export function prettyJson(raw: string | undefined | null): string {
  if (!raw || !raw.trim()) return '-'
  const s = raw.trim()

  // Single valid JSON -> pretty-print
  const pretty = tryPrettyJSON(s)
  if (pretty != null) return pretty

  // SSE dump: parse each data: line individually
  if (looksLikeSSE(s)) {
    const parts: string[] = []
    for (const data of iterateSSEData(s)) {
      parts.push(tryPrettyJSON(data) ?? data)
    }
    if (parts.length) return parts.join('\n\n---\n\n')
  }

  // Fallback: return raw text
  return s
}

/**
 * Parse a request/response body into per-role plain-text blocks for the Raw tab.
 * Each block has an uppercase role and the raw (non-markdown) content.
 * Returns null if the body cannot be parsed into role blocks.
 */
export type RawRoleBlock = { role: string; content: string }

export function extractRawRoleBlocks(raw: string | undefined | null): RawRoleBlock[] | null {
  if (!raw || !raw.trim()) return null
  let obj: any
  try { obj = JSON.parse(raw.trim()) } catch { return null }

  const blocks: RawRoleBlock[] = []
  if (obj.system != null) {
    const s = rawText(obj.system)
    if (s) blocks.push({ role: 'SYSTEM', content: s })
  }
  if (Array.isArray(obj.messages)) {
    for (const m of obj.messages) {
      const role = roleLabel(m.role)
      let content = rawText(m.content)
      const extras: string[] = []
      if (Array.isArray(m.tool_calls) && m.tool_calls.length) {
        for (const tc of m.tool_calls) extras.push(JSON.stringify(tc, null, 2))
      }
      if (m.tool_call_id) extras.push(`tool_call_id: ${m.tool_call_id}`)
      if (extras.length) {
        content = content ? `${content}\n\n${extras.join('\n\n')}` : extras.join('\n\n')
      }
      blocks.push({ role, content: content || '(empty)' })
    }
  }
  if (blocks.length === 0) return null
  return blocks
}

function rawText(content: unknown): string {
  if (content == null) return ''
  if (typeof content === 'string') return content
  return JSON.stringify(content, null, 2)
}

/**
 * Extract the plain concatenated text returned in a response body.
 * SSE streams: concatenate content deltas. JSON: extract assistant text.
 * Falls back to the raw string when not parseable.
 */
export function extractResponseText(raw: string | undefined | null): string {
  if (!raw || !raw.trim()) return ''
  const s = raw.trim()

  if (looksLikeSSE(s)) {
    return extractStreamText(s)
  }

  try {
    return extractResponseJSONText(JSON.parse(s))
  } catch {
    return s
  }
}

function extractStreamText(s: string): string {
  let text = ''
  let reasoning = ''
  for (const data of iterateSSEData(s)) {
    let obj: any
    try { obj = JSON.parse(data) } catch { continue }

    if (obj.object === 'chat.completion.chunk' || Array.isArray(obj.choices)) {
      const delta = obj.choices?.[0]?.delta
      if (delta && typeof delta.reasoning_content === 'string') reasoning += delta.reasoning_content
      if (delta && typeof delta.content === 'string') text += delta.content
      continue
    }
    const typ = obj.type || ''
    if (typ === 'content_block_delta') {
      const d = obj.delta || {}
      if (d.type === 'text_delta' && typeof d.text === 'string') text += d.text
    } else if (typ === 'content_block_start') {
      const cb = obj.content_block || {}
      if (cb.type === 'text' && typeof cb.text === 'string') text += cb.text
    }
  }
  return text || reasoning
}

function extractResponseJSONText(obj: any): string {
  if (Array.isArray(obj.choices)) {
    const parts: string[] = []
    for (const c of obj.choices) {
      const msg = c.message || c.delta || {}
      const t = plainContentText(msg.content)
      const reasoning = plainContentText(msg.reasoning_content)
      if (t) parts.push(t)
      else if (reasoning) parts.push(reasoning)
    }
    return parts.join('\n\n')
  }
  if (obj.type === 'message' || Array.isArray(obj.content)) {
    return plainContentText(obj.content)
  }
  if (obj.error) {
    return typeof obj.error === 'string'
      ? obj.error
      : obj.error.message || JSON.stringify(obj.error, null, 2)
  }
  return ''
}

function plainContentText(content: unknown): string {
  if (content == null) return ''
  if (typeof content === 'string') return content
  if (Array.isArray(content)) {
    const parts: string[] = []
    for (const p of content) {
      if (typeof p === 'string') { parts.push(p); continue }
      if (p && typeof p === 'object' && typeof p.text === 'string') parts.push(p.text)
    }
    return parts.join('\n\n')
  }
  return ''
}
