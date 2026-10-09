import { invoke } from '@tauri-apps/api/core'
import type {
  AppSettings,
  BackendError,
  ConnectivityResult,
  Project,
  RegistryConfig,
  ProjectSummary,
  StorageInfo,
  TestNetworkReport
} from '@/types/project'
import type { CatalogFile, MiddlewareTemplate } from '@/types/catalog'
import type { BuildResult } from '@/types/build'
import type { BuildHistoryEntry } from '@/types/buildHistory'
import type {
  CachedImage,
  EngineStatus,
  ImageInspect,
  PullResult
} from '@/types/images'
import type {
  ComposePluginStatus,
  DockerPkgEntry,
  ImportResult
} from '@/types/dockerPkgs'

/**
 * 后端适配层：前端唯一的后端调用入口。
 * 若未来切换桌面框架（如 Electron），仅需替换此文件实现。
 */
export const backend = {
  async validateProject(project: Project): Promise<import('@/utils/validate').ValidationIssue[]> {
    return invoke('validate_project', { project })
  },
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

  async cloneProject(id: string, newName: string): Promise<Project> {
    return invoke<Project>('clone_project', { id, newName })
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

  async saveCustomTemplate(template: MiddlewareTemplate, category: string): Promise<CatalogFile> {
    return invoke<CatalogFile>('save_custom_template', { template, category })
  },

  async deleteCustomTemplate(id: string): Promise<CatalogFile> {
    return invoke<CatalogFile>('delete_custom_template', { id })
  },

  async fetchRemoteCatalog(url: string): Promise<string> {
    return invoke<string>('fetch_remote_catalog', { url })
  },

  async listAuditLog(): Promise<string[]> {
    return invoke<string[]>('list_audit_log')
  },

  async restoreProjectFromBuild(dir: string): Promise<Project> {
    return invoke<Project>('restore_project_from_build', { dir })
  },

  async exportProject(id: string, outputPath: string): Promise<string> {
    return invoke<string>('export_project', { id, outputPath })
  },

  async importProject(inputPath: string): Promise<Project> {
    return invoke<Project>('import_project', { inputPath })
  },

  async analyzeCacheUsage(): Promise<
    { file: string; reference: string; platform: string; sizeBytes: number; referencedBy: string[] }[]
  > {
    return invoke('analyze_cache_usage')
  },

  async purgeUnrefCache(): Promise<[number, number, string[]]> {
    return invoke<[number, number, string[]]>('purge_unref_cache')
  },

  async listDockerVersions(arch: string): Promise<string[]> {
    return invoke<string[]>('list_docker_versions', { arch })
  },

  /** settings 传界面当前配置（未保存也可测）；缺省用已保存设置 */
  async networkTargets(settings: AppSettings): Promise<string[]> { return invoke('network_targets', { settings }) },
  async cancelTask(taskId: string): Promise<void> { return invoke('cancel_task', { taskId }) },
  async testNetwork(settings?: AppSettings, taskId?: string): Promise<TestNetworkReport> {
    return invoke<TestNetworkReport>('test_network', { settings: settings ?? null, taskId: taskId ?? null })
  },

  async engineStatus(): Promise<EngineStatus> {
    return invoke<EngineStatus>('engine_status')
  },

  async engineInstall(force: boolean, taskId?: string): Promise<EngineStatus> {
    return invoke<EngineStatus>('engine_install', { force, taskId: taskId ?? null })
  },

  async inspectImage(image: string, registry?: RegistryConfig): Promise<ImageInspect> {
    return invoke<ImageInspect>('inspect_image', { image, registry: registry ?? null })
  },

  async listImageTags(image: string, registry?: RegistryConfig): Promise<string[]> {
    return invoke<string[]>('list_image_tags', { image, registry: registry ?? null })
  },

  async listImageCache(): Promise<CachedImage[]> {
    return invoke<CachedImage[]>('list_image_cache')
  },

  async deleteCachedImage(file: string): Promise<void> {
    return invoke<void>('delete_cached_image', { file })
  },

  async pullImage(
    image: string,
    arch: string,
    registry?: RegistryConfig,
    taskId?: string
  ): Promise<PullResult> {
    return invoke<PullResult>('pull_image', { image, arch, registry: registry ?? null, taskId: taskId ?? null })
  },

  async listDockerPkgs(): Promise<DockerPkgEntry[]> {
    return invoke<DockerPkgEntry[]>('list_docker_pkgs')
  },

  async listComposePlugins(): Promise<ComposePluginStatus[]> {
    return invoke<ComposePluginStatus[]>('list_compose_plugins')
  },

  async deleteDockerPkg(dir: string): Promise<void> {
    return invoke<void>('delete_docker_pkg', { dir })
  },

  async importDockerPkgs(paths: string[], defaultArch: string, taskId?: string): Promise<ImportResult> {
    return invoke<ImportResult>('import_docker_pkgs', { paths, defaultArch, taskId: taskId ?? null })
  },

  async downloadDockerStatic(arch: string, version: string, taskId?: string): Promise<DockerPkgEntry> {
    return invoke<DockerPkgEntry>('download_docker_static', { arch, version, taskId: taskId ?? null })
  },

  async downloadComposePlugin(arch: string, taskId?: string): Promise<ComposePluginStatus> {
    return invoke<ComposePluginStatus>('download_compose_plugin', { arch, taskId: taskId ?? null })
  },

  async exportPortMatrixXlsx(project: Project, outputPath: string): Promise<string> {
    return invoke<string>('export_port_matrix_xlsx', { project, outputPath })
  },

  async listBuildHistory(): Promise<BuildHistoryEntry[]> {
    return invoke<BuildHistoryEntry[]>('list_build_history')
  },

  async deleteBuild(dir: string): Promise<void> {
    return invoke<void>('delete_build', { dir })
  },

  async openDirInExplorer(dir: string): Promise<void> {
    return invoke<void>('open_dir_in_explorer', { dir })
  },

  async backupAppData(outputPath: string): Promise<string> {
    return invoke<string>('backup_app_data', { outputPath })
  },

  async restoreAppData(backupPath: string): Promise<string> {
    return invoke<string>('restore_app_data', { backupPath })
  },

  async getDiskSpace(path: string): Promise<{ path: string; freeBytes: number; totalBytes: number }> {
    return invoke('get_disk_space', { path })
  },

  /** baselineBuildId 传空/null = 完整包；传基线构建号 = 增量升级包 */
  async buildOfflinePackage(
    project: Project,
    autoPull: boolean,
    baselineBuildId?: string | null,
    taskId?: string
  ): Promise<BuildResult> {
    return invoke<BuildResult>('build_offline_package', {
      project,
      autoPull,
      baselineBuildId: baselineBuildId ?? null,
      taskId: taskId ?? null
    })
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
