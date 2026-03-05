import { useState, useEffect } from 'react'
import { MOCK_CARDS } from '../mock-data'
import type { ActionCard } from '../types'

const BASE_URL = 'http://localhost:3000/v1/database/synapse-backend-g9cee'
const SQL_URL = `${BASE_URL}/sql`
const APPROVE_URL = `${BASE_URL}/call/approve_action`
const REJECT_URL = `${BASE_URL}/call/reject_action`
const ADD_COMMENT_URL = `${BASE_URL}/call/add_comment`

type SpacetimeSqlRow = unknown[]

interface SpacetimeSqlResponse {
  rows: SpacetimeSqlRow[]
}

function toNumber(value: unknown): number | null {
  if (typeof value === 'number') return value
  if (typeof value === 'string') {
    const n = Number(value)
    return Number.isFinite(n) ? n : null
  }
  return null
}

function mapDbRowToActionCard(row: SpacetimeSqlRow): ActionCard {
  const [
    id,
    agentId,
    _projectId,
    status,
    visualType,
    content,
    taskSummary,
    priority,
    _createdAt,
    _updatedAt,
  ] = row

  const numericId = toNumber(id) ?? 0
  const numericAgentId = toNumber(agentId) ?? 0

  const template =
    MOCK_CARDS[(numericAgentId > 0 ? numericAgentId - 1 : 0) % MOCK_CARDS.length]

  return {
    id: String(numericId || template.id),
    agentName: template.agentName,
    agentHandle: template.agentHandle,
    specialty: template.specialty,
    status:
      typeof status === 'string' && status
        ? (status as ActionCard['status'])
        : template.status,
    visualType:
      typeof visualType === 'string' && visualType
        ? (visualType as ActionCard['visualType'])
        : template.visualType,
    taskSummary:
      typeof taskSummary === 'string' && taskSummary
        ? taskSummary
        : template.taskSummary,
    content:
      typeof content === 'string' && content ? content : template.content,
    priority:
      typeof priority === 'number'
        ? priority
        : typeof priority === 'string'
          ? Number(priority) || template.priority
          : template.priority,
    concurrentTasks: template.concurrentTasks,
    tags: template.tags,
  }
}

export function useSpacetimeDB(): {
  cards: ActionCard[]
  isConnected: boolean
  error: Error | null
  approveCard: (id: number) => void
  rejectCard: (id: number) => void
  addComment: (id: number, text: string) => void
} {
  const [cards, setCards] = useState<ActionCard[]>(MOCK_CARDS)
  const [isConnected, setIsConnected] = useState(false)
  const [error, setError] = useState<Error | null>(null)

  useEffect(() => {
    let cancelled = false
    let intervalId: ReturnType<typeof setInterval> | null = null

    const useMockData = (err: Error | null) => {
      if (cancelled) return
      if (err) setError(err)
      setCards(MOCK_CARDS)
      setIsConnected(false)
    }

    const pollOnce = async () => {
      try {
        const response = await fetch(SQL_URL, {
          method: 'POST',
          headers: {
            'Content-Type': 'text/plain',
          },
          body:
            'SELECT id, agent_id, project_id, status, visual_type, content, task_summary, priority, created_at, updated_at FROM action_card ORDER BY priority DESC, id DESC',
        })

        if (!response.ok) {
          throw new Error(
            `SpacetimeDB SQL error: ${response.status} ${response.statusText}`,
          )
        }

        const data = (await response.json()) as SpacetimeSqlResponse

        if (!data || !Array.isArray(data.rows)) {
          throw new Error('SpacetimeDB SQL response missing rows')
        }

        const mapped = data.rows.map(mapDbRowToActionCard)

        if (!cancelled) {
          setCards(mapped)
          setIsConnected(true)
          setError(null)
        }
      } catch (e) {
        const err =
          e instanceof Error ? e : new Error('Failed to query SpacetimeDB')
        useMockData(err)
      }
    }

    void pollOnce()
    intervalId = setInterval(() => {
      void pollOnce()
    }, 5000)

    return () => {
      cancelled = true
      if (intervalId) clearInterval(intervalId)
    }
  }, [])

  const approveCard = (id: number) => {
    if (!isConnected) return
    void fetch(APPROVE_URL, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify([id]),
    }).catch((e) => {
      const err =
        e instanceof Error ? e : new Error('Failed to call approve_action')
      setError(err)
      setCards(MOCK_CARDS)
      setIsConnected(false)
    })
  }

  const rejectCard = (id: number) => {
    if (!isConnected) return
    void fetch(REJECT_URL, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify([id]),
    }).catch((e) => {
      const err =
        e instanceof Error ? e : new Error('Failed to call reject_action')
      setError(err)
      setCards(MOCK_CARDS)
      setIsConnected(false)
    })
  }

  const addComment = (id: number, text: string) => {
    if (!isConnected) return
    void fetch(ADD_COMMENT_URL, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify([id, text]),
    }).catch((e) => {
      const err =
        e instanceof Error ? e : new Error('Failed to call add_comment')
      setError(err)
      setCards(MOCK_CARDS)
      setIsConnected(false)
    })
  }

  return { cards, isConnected, error, approveCard, rejectCard, addComment }
}
