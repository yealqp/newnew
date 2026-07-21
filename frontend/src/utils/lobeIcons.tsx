/**
 * LobeHub icon registry — subset of ~80 AI/LLM provider icons
 * for the channel icon picker.
 */

// prettier-ignore
import {
  OpenAI, Anthropic, DeepSeek,
  Azure, AzureAI, Google, GoogleCloud, Gemini, VertexAI,
  Aws, Bedrock, Cloudflare,
  Mistral, Cohere, Together, Groq, Perplexity,
  Replicate, HuggingFace, Fireworks, LeptonAI,
  DeepInfra, SambaNova, Novita, OpenRouter,
  Ollama, Xinference, Vllm, LmStudio,
  Alibaba, AlibabaCloud, Qwen, Bailian,
  Baidu, BaiduCloud,
  Tencent, TencentCloud,
  ByteDance, Doubao,
  Zhipu, ChatGLM, Baichuan,
  Minimax, Moonshot, Kimi,
  Yi, ZeroOne, Spark, Stepfun,
  SiliconCloud, Huawei, HuaweiCloud,
  Nvidia, Meta, MetaAI, Microsoft,
  Stability, ElevenLabs, Cerebras,
  Grok, XAI, DeepL, Apple, IBM, Lambda, Anyscale,
  Coze, Dify, FastGPT,
  LangChain, Langfuse,
  OpenWebUI, CherryStudio,
  Cursor, ClaudeCode, Windsurf, NewAPI,
  Copilot, OpenCode, OpenHands,
  Cline, Replit, CodeGeeX,
  Vercel, Railway, Github,
  Notion,
} from '@lobehub/icons'

export type IconComponentType = React.ComponentType<{
  size?: number
  className?: string
  style?: React.CSSProperties
}>

export interface IconEntry {
  id: string
  title: string
  component: IconComponentType
}

// prettier-ignore
const _map: Record<string, IconComponentType> = {
  OpenAI, Anthropic, DeepSeek,
  Azure, AzureAI, Google, GoogleCloud, Gemini, VertexAI,
  Aws, Bedrock, Cloudflare,
  Mistral, Cohere, Together, Groq, Perplexity,
  Replicate, HuggingFace, Fireworks, LeptonAI,
  DeepInfra, SambaNova, Novita, OpenRouter,
  Ollama, Xinference, Vllm, LmStudio,
  Alibaba, AlibabaCloud, Qwen, Bailian,
  Baidu, BaiduCloud,
  Tencent, TencentCloud,
  ByteDance, Doubao,
  Zhipu, ChatGLM, Baichuan,
  Minimax, Moonshot, Kimi,
  Yi, ZeroOne, Spark, Stepfun,
  SiliconCloud, Huawei, HuaweiCloud,
  Nvidia, Meta, MetaAI, Microsoft,
  Stability, ElevenLabs, Cerebras,
  Grok, XAI, DeepL, Apple, IBM, Lambda, Anyscale,
  Coze, Dify, FastGPT,
  LangChain, Langfuse,
  OpenWebUI, CherryStudio,
  Cursor, ClaudeCode, Windsurf, NewAPI,
  Copilot, OpenCode, OpenHands,
  Cline, Replit, CodeGeeX,
  Vercel, Railway, Github,
  Notion,
}

export const ICON_MAP: Readonly<Record<string, IconComponentType>> = Object.freeze(_map)

export const ICON_LIST: IconEntry[] = Object.entries(ICON_MAP).map(([id, component]) => ({
  id,
  title: id,
  component,
}))

/** Render a lobehub icon by its string ID. Returns null if unknown. */
export function LobeIcon({
  id,
  size = 16,
  className,
}: {
  id?: string | null
  size?: number
  className?: string
}): React.ReactNode {
  if (!id) return null
  const Comp = ICON_MAP[id]
  if (!Comp) return null
  const ColorComp = (Comp as any)?.Color
  const Target = ColorComp || Comp
  return (
    <span style={{ display: 'inline-flex', verticalAlign: 'middle', lineHeight: 0 }} className={className}>
      <Target size={size} />
    </span>
  )
}
