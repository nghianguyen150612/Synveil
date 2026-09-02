import { useEffect, useRef } from 'react'
import { Link } from 'react-router-dom'

export function NotFoundPage() {
  const headingRef = useRef<HTMLHeadingElement>(null)
  useEffect(() => headingRef.current?.focus(), [])

  return (
    <section className="empty-state" aria-labelledby="not-found-title">
      <p className="eyebrow">404</p>
      <h1 id="not-found-title" ref={headingRef} tabIndex={-1}>That page does not exist</h1>
      <p>Check the address or return to the web foundation home.</p>
      <Link className="button" to="/">
        Return home
      </Link>
    </section>
  )
}
