// Thin wrappers around the Rust commands in app/src-tauri/src/commands.rs.
// Argument names are camelCase (Tauri converts them to the Rust snake_case names).
// A failed command rejects with the plain-language message string.

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { Choice, PlaylistInfo, Probed, QueueEvent } from './types';

export const missingTools = () => invoke<string[]>('missing_tools');
export const probeLink = (url: string) => invoke<Probed>('probe_link', { url });
export const addVideo = (url: string, title: string) => invoke<number>('add_video', { url, title });
export const addPlaylist = (playlist: PlaylistInfo) => invoke<number>('add_playlist', { playlist });
export const removeEntry = (id: number) => invoke<void>('remove_entry', { id });
export const removeGroup = (id: number) => invoke<void>('remove_group', { id });
export const startQueue = () => invoke<void>('start_queue');
export const stopQueue = () => invoke<void>('stop_queue');
export const clearFinished = () => invoke<void>('clear_finished');
export const getChoice = () => invoke<Choice>('get_choice');
export const setChoice = (choice: Choice) => invoke<void>('set_choice', { choice });
export const getFolder = () => invoke<string>('get_folder');
export const pickFolder = () => invoke<string | null>('pick_folder');
export const reveal = (path: string) => invoke<void>('reveal', { path });

export function onQueueEvent(cb: (e: QueueEvent) => void): Promise<UnlistenFn> {
  return listen<QueueEvent>('queue-event', (e) => cb(e.payload));
}
