import { describe, expect, it } from 'vitest'
import { formatBytes, formatDuration, formatEtaRange, formatSpeed } from './format'

describe('formatBytes', () => {
  it('uses whole bytes below 1 KB', () => {
    expect(formatBytes(0)).toBe('0 B')
    expect(formatBytes(999)).toBe('999 B')
  })

  it('steps up a unit every 1024', () => {
    expect(formatBytes(1024)).toBe('1.00 KB')
    expect(formatBytes(1024 * 1024)).toBe('1.00 MB')
    expect(formatBytes(1024 ** 3)).toBe('1.00 GB')
    expect(formatBytes(1024 ** 4)).toBe('1.00 TB')
  })

  it('uses two decimals below 10 and one above', () => {
    expect(formatBytes(1536)).toBe('1.50 KB')
    expect(formatBytes(1024 * 15)).toBe('15.0 KB')
  })

  it('stops at TB however big the number gets', () => {
    expect(formatBytes(1024 ** 5)).toBe('1024.0 TB')
  })

  it('treats impossible values as zero', () => {
    expect(formatBytes(-1)).toBe('0 B')
    expect(formatBytes(Number.NaN)).toBe('0 B')
    expect(formatBytes(Number.POSITIVE_INFINITY)).toBe('0 B')
  })
})

describe('formatSpeed', () => {
  it('adds the per-second unit', () => {
    expect(formatSpeed(1024 * 1024)).toBe('1.00 MB/s')
    expect(formatSpeed(0)).toBe('0 B/s')
  })
})

describe('formatDuration', () => {
  it('always gives two-digit hh:mm:ss', () => {
    expect(formatDuration(0)).toBe('00:00:00')
    expect(formatDuration(5)).toBe('00:00:05')
    expect(formatDuration(65)).toBe('00:01:05')
    expect(formatDuration(3661)).toBe('01:01:01')
  })

  it('does not round partial seconds up', () => {
    expect(formatDuration(1.9)).toBe('00:00:01')
  })

  it('goes past 24 h without resetting the hour count', () => {
    expect(formatDuration(90000)).toBe('25:00:00')
  })

  it('treats impossible values as zero', () => {
    expect(formatDuration(-10)).toBe('00:00:00')
    expect(formatDuration(Number.NaN)).toBe('00:00:00')
  })
})

describe('formatEtaRange', () => {
  it('promises no precision below half a minute', () => {
    expect(formatEtaRange(5, 20)).toBe('less than 30 s')
  })

  it('collapses the range when both ends round the same', () => {
    expect(formatEtaRange(120, 130)).toBe('2 min')
  })

  it('shows a range when the ends differ', () => {
    expect(formatEtaRange(120, 300)).toBe('2 min–5 min')
  })

  it('switches to hours and minutes past one hour', () => {
    expect(formatEtaRange(3600, 3600)).toBe('1 h')
    expect(formatEtaRange(3900, 3900)).toBe('1 h 5 min')
    expect(formatEtaRange(3600, 7200)).toBe('1 h–2 h')
  })

  it('avoids a rounded "0 min" on very short stretches', () => {
    expect(formatEtaRange(20, 40)).toBe('<1 min–1 min')
  })
})
