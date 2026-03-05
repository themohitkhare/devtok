import { Feed } from './components/Feed'
import { useSpacetimeDB } from './hooks/useSpacetimeDB'
import './index.css'

export default function App() {
  const { cards, isConnected } = useSpacetimeDB()
  return (
    <div style={{ position: 'relative' }}>
      <div
        style={{
          position: 'fixed',
          top: 12,
          left: 12,
          zIndex: 100,
          fontSize: 11,
          color: isConnected ? '#10b981' : '#6b7280',
          fontFamily: 'JetBrains Mono, monospace',
          background: 'rgba(0,0,0,0.6)',
          padding: '4px 8px',
          borderRadius: 6,
        }}
      >
        {isConnected ? '● live' : '● mock'}
      </div>
      <Feed cards={cards} />
    </div>
  )
}
