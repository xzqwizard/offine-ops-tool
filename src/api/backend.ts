import { invoke } from '@tauri-apps/api/core'
import type {
  AppSettings,
  BackendError,
  ConnectivityResult,
  Project,
  ProjectSummary,
  StorageInfo,
  TestNetworkReport
} from '@/types/project'
import type { CatalogFile } from '@/types/catalog'
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

  async listDockerVersions(arch: string): Promise<string[]> {
    return invoke<string[]>('list_docker_versions', { arch })
  },

  /** settings 传界面当前配置（未保存也可测）；缺省用已保存设置 */
  async testNetwork(settings?: AppSettings): Promise<TestNetworkReport> {
    return invoke<TestNetworkReport>('test_network', { settings: settings ?? null })
  },

  async engineStatus(): Promise<EngineStatus> {
    return invoke<EngineStatus>('engine_status')
  },

  async engineInstall(force: boolean): Promise<EngineStatus> {
    return invoke<EngineStatus>('engine_install', { force })
  },

  async inspectImage(image: string): Promise<ImageInspect> {
    return invoke<ImageInspect>('inspect_image', { image })
  },

  async listImageTags(image: string): Promise<string[]> {
    return invoke<string[]>('list_image_tags', { image })
  },

  async listImageCache(): Promise<CachedImage[]> {
    return invoke<CachedImage[]>('list_image_cache')
  },

  async deleteCachedImage(file: string): Promise<void> {
    return invoke<void>('delete_cached_image', { file })
  },

  async pullImage(image: string, arch: string): Promise<PullResult> {
    return invoke<PullResult>('pull_image', { image, arch })
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

  async importDockerPkgs(paths: string[], defaultArch: string): Promise<ImportResult> {
    return invoke<ImportResult>('import_docker_pkgs', { paths, defaultArch })
  },

  async downloadDockerStatic(arch: string, version: string): Promise<DockerPkgEntry> {
    return invoke<DockerPkgEntry>('download_docker_static', { arch, version })
  },

  async downloadComposePlugin(arch: string): Promise<ComposePluginStatus> {
    return invoke<ComposePluginStatus>('download_compose_plugin', { arch })
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

  /** baselineBuildId 传空/null = 完整包；传基线构建号 = 增量升级包 */
  async buildOfflinePackage(
    project: Project,
    autoPull: boolean,
    baselineBuildId?: string | null
  ): Promise<BuildResult> {
    return invoke<BuildResult>('build_offline_package', {
      project,
      autoPull,
      baselineBuildId: baselineBuildId ?? null
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
