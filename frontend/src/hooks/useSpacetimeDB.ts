import { useState, useEffect } from 'react'
import { MOCK_CARDS } from '../mock-data'
import type { ActionCard } from '../types'

const SPACETIME_WS_URL = 'ws://localhost:3000'

export interface UseSpacetimeDBResult {
  cards: ActionCard[]
  isConnected: boolean
  error: Error | null
}

/**
 * Connects to SpacetimeDB at ws://localhost:3000 with module 'synapse',
 * subscribes to ActionCard table. Falls back to mock data if connection fails.
 *
 * When @clockworklabs/spacetimedb-sdk is installed, use SpacetimeDBClient
 * to connect and subscribe to ActionCard; on disconnect or error set cards
 * to MOCK_CARDS and isConnected to false. Clean up client on unmount.
 */
export function useSpacetimeDB(): UseSpacetimeDBResult {
  const [cards, setCards] = useState<ActionCard[]>(MOCK_CARDS)
  const [isConnected, setIsConnected] = useState(false)
  const [error, setError] = useState<Error | null>(null)

  useEffect(() => {
    let cancelled = false
    let ws: WebSocket | null = null
    let timeoutId: ReturnType<typeof setTimeout> | null = null

    function useMockData() {
      if (!cancelled) {
        setCards(MOCK_CARDS)
        setIsConnected(false)
      }
    }

    try {
      ws = new WebSocket(SPACETIME_WS_URL)
      timeoutId = setTimeout(() => {
        if (!cancelled && ws?.readyState !== WebSocket.OPEN) {
          ws?.close()
          setError(new Error('Connection timeout'))
          useMockData()
        }
      }, 2500)

      ws.onopen = () => {
        if (timeoutId) clearTimeout(timeoutId)
        if (cancelled) return
        setError(null)
        // Without SpacetimeDB SDK we cannot subscribe to ActionCard table;
        // use mock data for development.
        ws?.close()
        useMockData()
      }

      ws.onerror = () => {
        if (timeoutId) clearTimeout(timeoutId)
        if (!cancelled) {
          setError(new Error('SpacetimeDB connection failed'))
          useMockData()
        }
      }

      ws.onclose = () => {
        if (timeoutId) clearTimeout(timeoutId)
        if (!cancelled) useMockData()
      }
    } catch (e) {
      if (timeoutId) clearTimeout(timeoutId)
      if (!cancelled) {
        setError(e instanceof Error ? e : new Error('Connection failed'))
        useMockData()
      }
    }

    return () => {
      cancelled = true
      if (timeoutId) clearTimeout(timeoutId)
      ws?.close()
    }
  }, [])

  return { cards, isConnected, error }
}
