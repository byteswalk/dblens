import { createContext, useContext } from 'react'

import type { Api } from '../shared/api'

export const ApiContext = createContext<Api | null>(null)

/** 当前的接口客户端（App 在拿到令牌后提供）。 */
export function useApi(): Api {
  const api = useContext(ApiContext)
  if (!api) throw new Error('useApi must be used inside ApiContext')
  return api
}
