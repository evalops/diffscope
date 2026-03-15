export function webhookSecret(env: NodeJS.ProcessEnv): string | undefined {
  return env.AUTOMATION_WEBHOOK_SECRET
}
