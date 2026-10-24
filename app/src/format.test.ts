import { describe, expect, it } from 'vitest'
import {
  copyProgress,
  formatBytes,
  formatCount,
  formatDuration,
  formatElapsed,
  formatEtaRange,
  formatList,
  formatWhen,
  formatSpeed,
  isFullPath,
  isRootPath,
  parseList,
  plural,
  relativePath,
  secondsLeft,
} from './format'

describe('formatWhen', () => {
  const today = new Date(2026, 9, 23, 21, 0)
  it('names today and yesterday', () => {
    expect(formatWhen('2026-10-23 20:09:07', today)).toBe('today 20:09')
    expect(formatWhen('2026-10-22 08:05:00', today)).toBe('yesterday 08:05')
  })

  it('gives the date otherwise, with the year only when it is not this one', () => {
    expect(formatWhen('2026-09-28 10:55:56', today)).toBe('28 Sept 10:55')
    expect(formatWhen('2025-12-31 23:59:59', today)).toBe('31 Dec 2025 23:59')
    expect(formatWhen('not a date', today)).toBe('not a date')
  })
})

describe('counts', () => {
  it('groups thousands and picks the singular for one', () => {
    expect(formatCount(12345)).toBe('12,345')
    expect(plural(1, 'file')).toBe('1 file')
    expect(plural(0, 'file')).toBe('0 files')
    expect(plural(2048, 'file or folder', 'files or folders')).toBe('2,048 files or folders')
  })
})

describe('formatElapsed', () => {
  it('keeps a tenth of a second while short, then minutes and hours', () => {
    expect(formatElapsed(420)).toBe('0.4 s')
    expect(formatElapsed(42_900)).toBe('42 s')
    expect(formatElapsed(185_000)).toBe('3 min 05 s')
    expect(formatElapsed(3_720_000)).toBe('1 h 02 min')
    expect(formatElapsed(-5)).toBe('0.0 s')
  })
})

describe('relativePath', () => {
  it('drops the part every path of the source shares', () => {
    expect(relativePath('C:\\COSAS', 'C:\\COSAS\\PROYECTOS\\app\\')).toBe('…\\PROYECTOS\\app')
    expect(relativePath('C:\\COSAS\\', 'c:\\cosas\\x')).toBe('…\\x')
  })

  it('leaves other paths whole', () => {
    expect(relativePath('C:\\COSAS', 'C:\\COSAS2\\x')).toBe('C:\\COSAS2\\x')
    expect(relativePath('C:\\COSAS', 'C:\\COSAS\\')).toBe('C:\\COSAS\\')
  })
})

describe('isRootPath', () => {
  it('spots a drive or share root, however it is written', () => {
    for (const p of ['D:\\', 'd:', '\\\\192.168.1.208\\copy', '\\\\nas\\share\\', '//nas/share']) {
      expect(isRootPath(p), p).toBe(true)
    }
  })

  it('leaves folders inside them alone', () => {
    for (const p of ['D:\\Backups', '\\\\192.168.1.208\\copy\\COSAS', '\\\\nas', '']) {
      expect(isRootPath(p), p).toBe(false)
    }
  })
})

describe('isFullPath', () => {
  it('takes a drive path or a network share', () => {
    for (const p of ['D:\\', 'D:/Backups', '  C:\\COSAS ', '\\\\nas\\share', '//nas/share/x']) {
      expect(isFullPath(p), p).toBe(true)
    }
  })

  it('rejects what Windows would resolve against another folder', () => {
    for (const p of ['\\nas\\share', 'backup', 'D:backup', 'D:', '\\\\nas', '']) {
      expect(isFullPath(p), p).toBe(false)
    }
  })
})

describe('copyProgress', () => {
  it('averages the share of bytes and of files done', () => {
    // 50 small files of 56 done, but almost none of the bytes.
    expect(copyProgress(341, 240_000_000, 50, 56)).toBeCloseTo(0.446, 3)
    expect(copyProgress(100, 100, 4, 4)).toBe(1)
  })

  it('uses whichever total is known', () => {
    expect(copyProgress(0, 0, 3, 4)).toBe(0.75)
    expect(copyProgress(50, 100, 0, 0)).toBe(0.5)
    expect(copyProgress(0, 0, 0, 0)).toBe(0)
  })
})

describe('secondsLeft', () => {
  it('extrapolates the pace of the window', () => {
    // 10% more in 10 s, 50% left: 50 s.
    const samples = [
      { tMs: 0, progress: 0.3 },
      { tMs: 10_000, progress: 0.4 },
      { tMs: 20_000, progress: 0.5 },
    ]
    expect(secondsLeft(samples, 10_000)).toBeCloseTo(50, 6)
    expect(secondsLeft(samples, 60_000)).toBeCloseTo(50, 6)
  })

  it('reads a run of small files as progress, unlike bytes alone', () => {
    // Robocopy's 50 notes (341 B of 240 MB) in the first second: the bytes
    // pace gave 313 h; the mixed one gives about a second.
    const at = (tMs: number, bytes: number, files: number) => ({
      tMs,
      progress: copyProgress(bytes, 240_000_000, files, 56),
    })
    expect(secondsLeft([at(0, 0, 0), at(1000, 341, 50)], 10_000)).toBeLessThan(2)
  })

  it('has no answer without a pace', () => {
    expect(secondsLeft([], 10_000)).toBe(0)
    expect(secondsLeft([{ tMs: 0, progress: 0.2 }], 10_000)).toBe(0)
    expect(
      secondsLeft(
        [
          { tMs: 0, progress: 0.2 },
          { tMs: 5000, progress: 0.2 },
        ],
        10_000,
      ),
    ).toBe(0)
  })
})

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

describe('parseList', () => {
  it('splits on any run of spaces', () => {
    expect(parseList('  .git   node_modules ')).toEqual(['.git', 'node_modules'])
    expect(parseList('')).toEqual([])
  })

  it('keeps a quoted entry with spaces whole', () => {
    expect(parseList('.git "System Volume Information" *.tmp')).toEqual([
      '.git',
      'System Volume Information',
      '*.tmp',
    ])
  })

  it('reads a quote still being typed up to the end', () => {
    expect(parseList('.git "System Vol')).toEqual(['.git', 'System Vol'])
  })

  it('round-trips with formatList', () => {
    const items = ['.git', 'System Volume Information', '$RECYCLE.BIN']
    expect(formatList(items)).toBe('.git "System Volume Information" $RECYCLE.BIN')
    expect(parseList(formatList(items))).toEqual(items)
  })
})
