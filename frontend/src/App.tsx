import { Feed } from './components/Feed'
import { MOCK_CARDS } from './mock-data'
import './index.css'

export default function App() {
  return <Feed cards={MOCK_CARDS} />
}
