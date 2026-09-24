import { useState } from 'react'
import type { CopyMode, Engine, Profile } from '../types'
import { emptyRestic, newProfile } from '../types'
import { open } from '@tauri-apps/plugin-dialog'

interface Props {
  initial?: Profile
  onCancel: () => void
  onSave: (profile: Profile) => void
}

const MODE_INFO: Record<CopyMode, { label: string; result: string; tone: 'safe' | 'danger' }> = {
  accumulate: {
    label: 'Add only',
    result: 'Adds and updates. Never deletes anything in the destination, even if you delete it in the source.',
    tone: 'safe',
  },
  mirror: {
    label: 'Mirror',
    result:
      'Leaves the destination IDENTICAL to the source: whatever you delete in the source is deleted in the destination too.',
    tone: 'danger',
  },
}

const ENGINE_INFO: Record<Engine, { label: string; result: string }> = {
  robocopy: {
    label: 'Direct copy (fast)',
    result: 'Copies files as they are. Fast and with no extra dependencies, but with no history: each copy replaces the previous one.',
  },
  restic: {
    label: 'Versioned (encrypted snapshots)',
    result:
      'Every copy is kept as its own version, encrypted and compressed. Needs "restic" installed (winget install restic.restic) and a password, which is asked for on every run — it is never stored.',
  },
}

export function ProfileForm({ initial, onCancel, onSave }: Props) {
  const [profile, setProfile] = useState<Profile>(initial ?? newProfile())
  const [needsCreds, setNeedsCreds] = useState(!!initial?.credentials)
  const [needsWol, setNeedsWol] = useState(!!initial?.wake_on_lan)
  const [showAdvanced, setShowAdvanced] = useState(false)

  const canSave = profile.name.trim() && profile.source.trim() && profile.destination.trim()

  const pickFolder = async (which: 'source' | 'destination') => {
    const picked = await open({ directory: true, multiple: false })
    if (typeof picked === 'string') {
      setProfile((p) => ({ ...p, [which]: picked }))
    }
  }

  const setAdv = <K extends keyof Profile['advanced']>(key: K, value: Profile['advanced'][K]) =>
    setProfile((p) => ({ ...p, advanced: { ...p.advanced, [key]: value } }))

  const submit = () => {
    const toSave: Profile = {
      ...profile,
      credentials: needsCreds ? { user: profile.credentials?.user ?? '', password: '' } : null,
      wake_on_lan: needsWol
        ? profile.wake_on_lan ?? { mac: '', host: '', timeout_secs: 120 }
        : null,
    }
    onSave(toSave)
  }

  return (
    <div className="profile-form">
      <h2>{initial ? 'Edit job' : 'New backup job'}</h2>

      <label className="field">
        <span>Job name</span>
        <input
          value={profile.name}
          placeholder="e.g. Photos to the NAS"
          onChange={(e) => setProfile((p) => ({ ...p, name: e.target.value }))}
        />
      </label>

      <label className="field">
        <span>Source</span>
        <div className="field-row">
          <input
            value={profile.source}
            placeholder="C:\Users\you\Documents"
            onChange={(e) => setProfile((p) => ({ ...p, source: e.target.value }))}
          />
          <button type="button" aria-label="Browse for the source folder" onClick={() => pickFolder('source')}>
            Browse
          </button>
        </div>
      </label>

      <label className="field">
        <span>Destination</span>
        <div className="field-row">
          <input
            value={profile.destination}
            placeholder="\\nas\backup\Documents"
            onChange={(e) => setProfile((p) => ({ ...p, destination: e.target.value }))}
          />
          <button type="button" aria-label="Browse for the destination folder" onClick={() => pickFolder('destination')}>
            Browse
          </button>
        </div>
      </label>

      <fieldset className="mode-picker">
        <legend>Copy mode</legend>
        {(Object.keys(MODE_INFO) as CopyMode[]).map((mode) => (
          <label key={mode} className={`mode-option mode-option--${MODE_INFO[mode].tone}`}>
            <input
              type="radio"
              name="mode"
              checked={profile.mode === mode}
              onChange={() => setProfile((p) => ({ ...p, mode }))}
            />
            <div>
              <strong>{MODE_INFO[mode].label}</strong>
              <p>{MODE_INFO[mode].result}</p>
            </div>
          </label>
        ))}
      </fieldset>

      <fieldset className="mode-picker">
        <legend>Copy method</legend>
        {(Object.keys(ENGINE_INFO) as Engine[]).map((engine) => (
          <label key={engine} className="mode-option mode-option--safe">
            <input
              type="radio"
              name="engine"
              checked={profile.engine === engine}
              onChange={() =>
                setProfile((p) => ({
                  ...p,
                  engine,
                  restic: p.restic ?? emptyRestic(),
                }))
              }
            />
            <div>
              <strong>{ENGINE_INFO[engine].label}</strong>
              <p>{ENGINE_INFO[engine].result}</p>
            </div>
          </label>
        ))}
      </fieldset>

      {profile.engine === 'restic' && (
        <div className="advanced-grid indent">
          <label>
            Daily versions to keep
            <input
              type="number"
              min={0}
              value={profile.restic.keep_daily}
              onChange={(e) =>
                setProfile((p) => ({
                  ...p,
                  restic: { ...p.restic, keep_daily: Number(e.target.value) },
                }))
              }
            />
          </label>
          <label>
            Weekly versions to keep
            <input
              type="number"
              min={0}
              value={profile.restic.keep_weekly}
              onChange={(e) =>
                setProfile((p) => ({
                  ...p,
                  restic: { ...p.restic, keep_weekly: Number(e.target.value) },
                }))
              }
            />
          </label>
          <label>
            Monthly versions to keep
            <input
              type="number"
              min={0}
              value={profile.restic.keep_monthly}
              onChange={(e) =>
                setProfile((p) => ({
                  ...p,
                  restic: { ...p.restic, keep_monthly: Number(e.target.value) },
                }))
              }
            />
          </label>
        </div>
      )}

      <label className="field field--checkbox">
        <input type="checkbox" checked={needsCreds} onChange={(e) => setNeedsCreds(e.target.checked)} />
        <span>The network destination asks for a user name and password</span>
      </label>
      {needsCreds && (
        <div className="field-row indent">
          <input
            placeholder="User name"
            value={profile.credentials?.user ?? ''}
            onChange={(e) =>
              setProfile((p) => ({
                ...p,
                credentials: { user: e.target.value, password: '' },
              }))
            }
          />
          <span className="field-hint">The password is asked for on every run: it is never stored.</span>
        </div>
      )}

      <label className="field field--checkbox">
        <input type="checkbox" checked={needsWol} onChange={(e) => setNeedsWol(e.target.checked)} />
        <span>Wake the destination over the network (Wake-on-LAN) if it is off</span>
      </label>
      {needsWol && (
        <div className="field-row indent">
          <input
            placeholder="MAC: AA:BB:CC:DD:EE:FF"
            value={profile.wake_on_lan?.mac ?? ''}
            onChange={(e) =>
              setProfile((p) => ({
                ...p,
                wake_on_lan: { mac: e.target.value, host: p.wake_on_lan?.host ?? '', timeout_secs: 120 },
              }))
            }
          />
          <input
            placeholder="Host/IP to check it woke up (optional)"
            value={profile.wake_on_lan?.host ?? ''}
            onChange={(e) =>
              setProfile((p) => ({
                ...p,
                wake_on_lan: { mac: p.wake_on_lan?.mac ?? '', host: e.target.value, timeout_secs: 120 },
              }))
            }
          />
        </div>
      )}

      <button
        type="button"
        className="link-btn"
        aria-expanded={showAdvanced}
        aria-controls="advanced-options"
        onClick={() => setShowAdvanced((v) => !v)}
      >
        <span className="caret" aria-hidden="true">
          ▸
        </span>{' '}
        Advanced options
      </button>
      {/* Always kept in the DOM so the expansion can be animated; `inert`
          removes it from the tab order and from screen readers while
          collapsed. */}
      <div className="collapse" id="advanced-options" data-open={showAdvanced} inert={!showAdvanced}>
        <div className="collapse__clip">
          <div className="advanced-grid">
              <label>
                Copy speed (threads)
                <input
                  type="number"
                  min={1}
                  max={128}
                  value={profile.advanced.threads}
                  onChange={(e) => setAdv('threads', Number(e.target.value))}
                />
              </label>
              <label>
                Retries on failure
                <input
                  type="number"
                  min={0}
                  value={profile.advanced.retries}
                  onChange={(e) => setAdv('retries', Number(e.target.value))}
                />
              </label>
              <label>
                Wait between retries (seconds)
                <input
                  type="number"
                  min={0}
                  value={profile.advanced.wait_secs}
                  onChange={(e) => setAdv('wait_secs', Number(e.target.value))}
                />
              </label>
              <label className="field--checkbox">
                <input
                  type="checkbox"
                  checked={profile.advanced.unbuffered_io}
                  onChange={(e) => setAdv('unbuffered_io', e.target.checked)}
                />
                Fast mode for large files
              </label>
              <label className="field--checkbox">
                <input
                  type="checkbox"
                  checked={profile.advanced.fat_time_tolerance}
                  onChange={(e) => setAdv('fat_time_tolerance', e.target.checked)}
                />
                Avoid recopying on network drives (recommended for NAS)
              </label>
              <label>
                Exclude folders
                <input
                  placeholder="Thumbs.db .git node_modules"
                  value={profile.advanced.exclude_dirs.join(' ')}
                  onChange={(e) => setAdv('exclude_dirs', e.target.value.split(/\s+/).filter(Boolean))}
                />
              </label>
              <label>
                Exclude files
                <input
                  placeholder="*.tmp *.log desktop.ini"
                  value={profile.advanced.exclude_files.join(' ')}
                  onChange={(e) => setAdv('exclude_files', e.target.value.split(/\s+/).filter(Boolean))}
                />
              </label>
          </div>
        </div>
      </div>

      <div className="form-actions">
        <button type="button" onClick={onCancel}>
          Cancel
        </button>
        <button type="button" className="primary" disabled={!canSave} onClick={submit}>
          Save job
        </button>
      </div>
    </div>
  )
}
