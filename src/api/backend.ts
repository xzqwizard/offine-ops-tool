import { invoke } from '@tauri-apps/api/core'
import type {
  AppSettings,
  BackendError,
  Project,
  ProjectSummary,
  StorageInfo
} from '@/types/project'
import type { CatalogFile } from '@/types/catalog'
import type { BuildResult } from '@/types/build'

/**
 * 后端适配层：前端唯一的后端调用入口。
 * 若未来切换桌面框架（如 Electron），仅需替换此文件实现。
 */
export const backend = {
  async listProjects(): Promise<ProjectSummary[]> {
    return invoke<ProjectSummary[]>('list_projects')
  },

  async createProject(name: string, customer: string): Promise<Project> {
    return invoke<Project>('create_project', { name, customer })
  },

  async saveProject(project: Project): Promise<Project> {
    return invoke<Project>('save_project', { project })
  },

  async loadProject(id: string): Promise<Project> {
    return invoke<Project>('load_project', { id })
  },

  async deleteProject(id: string): Promise<void> {
    return invoke<void>('delete_project', { id })
  },

  async getSettings(): Promise<AppSettings> {
    return invoke<AppSettings>('get_settings')
  },

  async saveSettings(settings: AppSettings): Promise<StorageInfo> {
    return invoke<StorageInfo>('save_settings', { settings })
  },

  async getStorageInfo(): Promise<StorageInfo> {
    return invoke<StorageInfo>('get_storage_info')
  },

  async listCatalog(): Promise<CatalogFile> {
    return invoke<CatalogFile>('list_catalog')
  },

  async buildOfflinePackage(project: Project): Promise<BuildResult> {
    return invoke<BuildResult>('build_offline_package', { project })
  }
}

/** 把 Tauri invoke 抛出的 AppError 转成 Error（保留 kind 信息） */
export function toAppError(e: unknown): Error {
  const be = e as BackendError
  if (be && typeof be === 'object' && typeof be.message === 'string') {
    const err = new Error(be.message) as Error & { kind?: string }
    err.kind = be.kind
    return err
  }
  return e instanceof Error ? e : new Error(String(e))
}
