export interface ModelPreset {
  id: string
  label: string
  ctx: string
  price: string
}

export const MODEL_PRESETS: ModelPreset[] = [
  { id: 'openai/gpt-5.4-pro', label: 'GPT-5.4 Pro', ctx: '1.1M', price: '$30' },
  { id: 'anthropic/claude-opus-4.6', label: 'Claude Opus 4.6', ctx: '1M', price: '$5' },
  { id: 'openai/gpt-5.4', label: 'GPT-5.4', ctx: '1.1M', price: '$2.50' },
  { id: 'anthropic/claude-sonnet-4.6', label: 'Claude Sonnet 4.6', ctx: '1M', price: '$3' },
  { id: 'google/gemini-3.1-pro-preview', label: 'Gemini 3.1 Pro', ctx: '1M', price: '$2' },
  { id: 'openai/gpt-5.3-codex', label: 'GPT-5.3 Codex', ctx: '400K', price: '$1.75' },
  { id: 'openai/gpt-5.2-codex', label: 'GPT-5.2 Codex', ctx: '400K', price: '$1.75' },
  { id: 'mistralai/devstral-2512', label: 'Devstral', ctx: '262K', price: '$0.40' },
  { id: 'qwen/qwen3-coder-next', label: 'Qwen3 Coder', ctx: '262K', price: '$0.12' },
  { id: 'deepseek/deepseek-v3.2', label: 'DeepSeek V3.2', ctx: '163K', price: '$0.25' },
  { id: 'meta-llama/llama-4-maverick', label: 'Llama 4 Maverick', ctx: '1M', price: '$0.15' },
  { id: 'meta-llama/llama-4-scout', label: 'Llama 4 Scout', ctx: '327K', price: '$0.08' },
  { id: 'qwen/qwen3.5-flash-02-23', label: 'Qwen3.5 Flash', ctx: '1M', price: '$0.10' },
  { id: 'google/gemini-3-flash-preview', label: 'Gemini 3 Flash', ctx: '1M', price: '$0.50' },
  { id: 'google/gemini-3.1-flash-lite-preview', label: 'Gemini 3.1 Flash Lite', ctx: '1M', price: '$0.25' },
  { id: 'qwen/qwen3-coder-next:free', label: 'Qwen3 Coder (free)', ctx: '262K', price: 'free' },
  { id: 'nvidia/nemotron-3-nano-30b-a3b:free', label: 'Nemotron 3 Nano (free)', ctx: '256K', price: 'free' },
]
