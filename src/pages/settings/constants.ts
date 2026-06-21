import type { LLMProtocol } from '@/lib/skill-types'

type ProviderPreset = {
  id: string
  label: string
  category: string
  protocol: LLMProtocol
  base_url: string
  models: string[]
  api_key_placeholder?: string
  note?: string
}

export const PROVIDER_PRESETS: ProviderPreset[] = [
  {
    id: 'openai',
    label: 'OpenAI',
    category: '国际',
    protocol: 'openai',
    base_url: 'https://api.openai.com/v1',
    models: ['gpt-4.1', 'gpt-4o', 'gpt-4o-mini'],
    api_key_placeholder: 'sk-...',
  },
  {
    id: 'anthropic',
    label: 'Anthropic Claude',
    category: '国际',
    protocol: 'anthropic',
    base_url: 'https://api.anthropic.com/v1',
    models: ['claude-sonnet-4-20250514', 'claude-3-5-sonnet-20241022'],
    api_key_placeholder: 'sk-ant-...',
  },
  {
    id: 'google-gemini',
    label: 'Google Gemini',
    category: '国际',
    protocol: 'openai',
    base_url: 'https://generativelanguage.googleapis.com/v1beta/openai',
    models: ['gemini-2.5-pro', 'gemini-2.5-flash'],
  },
  {
    id: 'deepseek',
    label: 'DeepSeek',
    category: '国内',
    protocol: 'openai',
    base_url: 'https://api.deepseek.com/v1',
    models: ['deepseek-chat', 'deepseek-reasoner'],
  },
  {
    id: 'dashscope',
    label: '阿里云百炼 / 通义千问',
    category: '国内',
    protocol: 'openai',
    base_url: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
    models: ['qwen-plus', 'qwen-max', 'qwen3-235b-a22b'],
  },
  {
    id: 'zhipu',
    label: '智谱 AI',
    category: '国内',
    protocol: 'openai',
    base_url: 'https://open.bigmodel.cn/api/paas/v4',
    models: ['glm-4-plus', 'glm-4-air', 'glm-4-flash'],
  },
  {
    id: 'moonshot',
    label: 'Moonshot / Kimi',
    category: '国内',
    protocol: 'openai',
    base_url: 'https://api.moonshot.cn/v1',
    models: ['moonshot-v1-128k', 'moonshot-v1-32k', 'kimi-k2-0711-preview'],
  },
  {
    id: 'siliconflow',
    label: '硅基流动',
    category: '国内',
    protocol: 'openai',
    base_url: 'https://api.siliconflow.cn/v1',
    models: ['deepseek-ai/DeepSeek-V3', 'deepseek-ai/DeepSeek-R1', 'Qwen/Qwen3-235B-A22B'],
  },
  {
    id: 'minimax-cn',
    label: 'MiniMax（中国）',
    category: '国内',
    protocol: 'openai',
    base_url: 'https://api.minimax.chat/v1',
    models: ['MiniMax-Text-01', 'abab6.5s-chat', 'abab6.5g-chat'],
  },
  {
    id: 'baichuan',
    label: '百川智能',
    category: '国内',
    protocol: 'openai',
    base_url: 'https://api.baichuan-ai.com/v1',
    models: ['Baichuan4', 'Baichuan3-Turbo'],
  },
  {
    id: 'openrouter',
    label: 'OpenRouter',
    category: '聚合网关',
    protocol: 'openai',
    base_url: 'https://openrouter.ai/api/v1',
    models: ['anthropic/claude-sonnet-4', 'openai/gpt-4o', 'deepseek/deepseek-chat'],
  },
  {
    id: 'vercel-ai-gateway',
    label: 'Vercel AI Gateway',
    category: '聚合网关',
    protocol: 'openai',
    base_url: 'https://ai-gateway.vercel.sh/v1',
    models: ['openai/gpt-4o', 'anthropic/claude-sonnet-4', 'google/gemini-2.5-pro'],
  },
  {
    id: 'portkey',
    label: 'Portkey',
    category: '聚合网关',
    protocol: 'openai',
    base_url: 'https://api.portkey.ai/v1',
    models: ['gpt-4o', '@anthropic-prod/claude-sonnet-4-20250514'],
  },
  {
    id: 'groq',
    label: 'Groq',
    category: '国际',
    protocol: 'openai',
    base_url: 'https://api.groq.com/openai/v1',
    models: ['llama-3.3-70b-versatile', 'deepseek-r1-distill-llama-70b'],
  },
  {
    id: 'mistral',
    label: 'Mistral AI',
    category: '国际',
    protocol: 'openai',
    base_url: 'https://api.mistral.ai/v1',
    models: ['mistral-large-latest', 'codestral-latest'],
  },
  {
    id: 'xai',
    label: 'xAI',
    category: '国际',
    protocol: 'openai',
    base_url: 'https://api.x.ai/v1',
    models: ['grok-3', 'grok-3-mini'],
  },
  {
    id: 'together',
    label: 'Together AI',
    category: '国际',
    protocol: 'openai',
    base_url: 'https://api.together.xyz/v1',
    models: ['meta-llama/Llama-3.3-70B-Instruct-Turbo', 'deepseek-ai/DeepSeek-R1'],
  },
  {
    id: 'fireworks',
    label: 'Fireworks AI',
    category: '国际',
    protocol: 'openai',
    base_url: 'https://api.fireworks.ai/inference/v1',
    models: [
      'accounts/fireworks/models/llama-v3p1-405b-instruct',
      'accounts/fireworks/models/deepseek-r1',
    ],
  },
  {
    id: 'nvidia',
    label: 'NVIDIA NIM',
    category: '国际',
    protocol: 'openai',
    base_url: 'https://integrate.api.nvidia.com/v1',
    models: ['nvidia/llama-3.1-nemotron-70b-instruct', 'deepseek-ai/deepseek-r1'],
  },
  {
    id: 'ollama',
    label: 'Ollama 本地',
    category: '本地',
    protocol: 'local',
    base_url: 'http://localhost:11434',
    models: ['qwen2.5:7b', 'llama3.1:8b', 'deepseek-r1:8b'],
  },
  {
    id: 'lm-studio',
    label: 'LM Studio / vLLM',
    category: '本地',
    protocol: 'openai',
    base_url: 'http://localhost:1234/v1',
    models: ['local-model'],
    api_key_placeholder: '任意非空值',
  },
  {
    id: 'custom-openai',
    label: '自定义 OpenAI 兼容',
    category: '自定义',
    protocol: 'openai',
    base_url: '',
    models: [''],
  },
  {
    id: 'custom-anthropic',
    label: '自定义 Anthropic 兼容',
    category: '自定义',
    protocol: 'anthropic',
    base_url: '',
    models: [''],
  },
  {
    id: 'custom-local',
    label: '自定义 Ollama 原生',
    category: '自定义',
    protocol: 'local',
    base_url: 'http://localhost:11434',
    models: ['local-model'],
  },
]

export const DEFAULT_PROVIDER_PRESET_ID = 'openai'

export const PROVIDER_DEFAULTS: Record<LLMProtocol, { base_url: string; model: string }> = {
  openai: {
    base_url: 'https://api.openai.com/v1',
    model: 'gpt-4.1',
  },
  anthropic: {
    base_url: 'https://api.anthropic.com/v1',
    model: 'claude-sonnet-4-20250514',
  },
  local: {
    base_url: 'http://localhost:11434',
    model: 'qwen2.5:7b',
  },
}

export function providerModelsText(preset: ProviderPreset): string {
  return preset.models.filter(Boolean).join('\n')
}
