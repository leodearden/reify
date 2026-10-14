import { getQueriesForElement, screen } from '@solidjs/testing-library'
import { DelegatedEvents, delegateEvents } from 'solid-js/web'
import { vi } from 'vitest'

// Restores, for each test file, the baseline a fresh fork used to provide when
// the worker process and its module cache are reused (task 7642). Cached
// libraries capture the FIRST file's jsdom at import: testing-library's
// `screen`, solid testing-library's afterEach(cleanup), and solid's
// delegateEvents in compiled components. See docs/notes/gui-vitest-shared-workers.md.

export function rebindDomCapturesTo(doc: Document): void {
  Object.assign(screen, getQueriesForElement(doc.body))
  delegateEvents([...DelegatedEvents], doc)
}

export function restoreGlobalBaseline(): void {
  vi.useRealTimers()
  vi.unstubAllGlobals()
  vi.unstubAllEnvs()
}
