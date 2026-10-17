import { AuthdogProvider } from "@authdog/nextjs-app/client"

export function AuthdogRoot({ children }: { children: React.ReactNode }) {
  return <AuthdogProvider>{children}</AuthdogProvider>
}
