import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type { HistoryEntry, JobEvent, JobRequest, Profile } from './types'

const EVENT_CHANNEL = 'nasmirror://job'

export const api = {
  listProfiles: () => invoke<Profile[]>('list_profiles'),
  saveProfile: (profile: Profile) => invoke<Profile[]>('save_profile', { profile }),
  deleteProfile: (id: string) => invoke<Profile[]>('delete_profile', { id }),
  openLogDir: () => invoke<void>('open_log_dir'),
  listHistory: () => invoke<HistoryEntry[]>('list_history'),
  readLog: (id: string) => invoke<string>('read_log', { id }),
  deleteHistoryEntry: (id: string) => invoke<HistoryEntry[]>('delete_history_entry', { id }),
  startJob: (request: JobRequest) => invoke<string>('start_job', { request }),
  cancelJob: (jobId: string) => invoke<boolean>('cancel_job', { jobId }),
}

export function onJobEvent(handler: (event: JobEvent) => void): Promise<UnlistenFn> {
  return listen<JobEvent>(EVENT_CHANNEL, (e) => handler(e.payload))
}
