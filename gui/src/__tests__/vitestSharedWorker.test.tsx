import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { screen } from '@solidjs/testing-library'
import { render } from 'solid-js/web'
import { rebindDomCapturesTo, restoreGlobalBaseline } from '../../vitest-shared-worker'

describe('rebindDomCapturesTo', () => {
  let other: Document
  let disposers: Array<() => void>

  beforeEach(() => {
    other = document.implementation.createHTMLDocument('other')
    disposers = []
  })

  afterEach(() => {
    for (const dispose of disposers) dispose()
    rebindDomCapturesTo(document)
    document.body.replaceChildren()
  })

  const mountButtonIn = (doc: Document, onClick: () => void): HTMLButtonElement => {
    const container = doc.createElement('div')
    doc.body.appendChild(container)
    disposers.push(
      render(
        () => (
          <button data-testid="b" onClick={onClick}>
            x
          </button>
        ),
        container,
      ),
    )
    return container.querySelector('button')!
  }

  const click = (target: Element) => target.dispatchEvent(new MouseEvent('click', { bubbles: true }))

  it('points screen queries at the given document body', () => {
    const inOther = other.createElement('div')
    inOther.dataset.testid = 'only-in-other'
    other.body.appendChild(inOther)
    const inGlobal = document.createElement('div')
    inGlobal.dataset.testid = 'only-in-global'
    document.body.appendChild(inGlobal)

    rebindDomCapturesTo(other)

    expect(screen.getByTestId('only-in-other')).toBe(inOther)
    expect(screen.queryByTestId('only-in-global')).toBeNull()
  })

  it('makes solid delegated events reach handlers in a document nothing delegated on', () => {
    const onClick = vi.fn()
    const button = mountButtonIn(other, onClick)

    click(button)
    expect(onClick).not.toHaveBeenCalled()

    rebindDomCapturesTo(other)
    click(button)
    expect(onClick).toHaveBeenCalledTimes(1)
  })

  it('is idempotent: rebinding twice does not double-fire a handler', () => {
    const onClick = vi.fn()
    const button = mountButtonIn(other, onClick)

    rebindDomCapturesTo(other)
    rebindDomCapturesTo(other)
    click(button)

    expect(onClick).toHaveBeenCalledTimes(1)
  })
})

describe('restoreGlobalBaseline', () => {
  it('removes a global stubbed with vi.stubGlobal', () => {
    vi.stubGlobal('__vitestSharedWorkerProbe', 42)

    restoreGlobalBaseline()

    expect('__vitestSharedWorkerProbe' in globalThis).toBe(false)
  })

  it('removes an env var stubbed with vi.stubEnv', () => {
    vi.stubEnv('VITEST_SHARED_WORKER_PROBE', 'x')

    restoreGlobalBaseline()

    expect(process.env.VITEST_SHARED_WORKER_PROBE).toBeUndefined()
  })

  it('switches fake timers back to real ones', () => {
    vi.useFakeTimers()

    restoreGlobalBaseline()

    expect(vi.isFakeTimers()).toBe(false)
  })
})
