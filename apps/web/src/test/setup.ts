import '@testing-library/jest-dom/vitest'
import { afterEach, vi } from 'vitest'

// The host shell exports NODE_ENV=production. Force the test runtime to load
// React's test-safe development build before the testing library is imported.
vi.stubEnv('NODE_ENV', 'test')

const { cleanup } = await import('@testing-library/react')

afterEach(() => {
  cleanup()
})
