// Thin wrappers around the Rust commands in app/src-tauri/src/commands.rs.
// Argument names are camelCase (Tauri converts them to the Rust snake_case names).
// A failed command rejects with the plain-language message string.

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { DownloadJob, JobEvent, VideoInfo } from './types';

export const missingTools = () => invoke<string[]>('missing_tools');
export const probe = (url: string) => invoke<VideoInfo>('probe', { url });
export const startDownload = (job: DownloadJob) => invoke<number>('start_download', { job });
export const cancelDownload = (jobId: number) => invoke<void>('cancel_download', { jobId });
export const getFolder = () => invoke<string>('get_folder');
export const pickFolder = () => invoke<string | null>('pick_folder');
export const reveal = (path: string) => invoke<void>('reveal', { path });

export function onJobEvent(cb: (e: JobEvent) => void): Promise<UnlistenFn> {
  return listen<JobEvent>('job-event', (e) => cb(e.payload));
}
