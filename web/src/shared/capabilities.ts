import { useQuery } from '@tanstack/react-query'

export type Capabilities = { edition: 'standalone' | 'audit'; features: string[] }

export const STANDALONE: Capabilities = { edition: 'standalone', features: [] }

export function hasFeature(c: Capabilities, name: string): boolean {
  return c.features.includes(name)
}

/** 版本和能力。加载完成前按单机版显示。 */
export function useCapabilities(api: { capabilities(): Promise<Capabilities> }): Capabilities {
  const { data } = useQuery({ queryKey: ['capabilities'], queryFn: () => api.capabilities(), staleTime: Infinity })
  return data ?? STANDALONE
}
