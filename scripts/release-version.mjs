import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const cargoVersion = /(^\[package\]\r?\n(?:(?!^\[)[\s\S])*?^version\s*=\s*")([^"]+)(")/m
const lockVersion = /(^\[\[package\]\]\r?\nname = "offline-preops-tool"\r?\nversion = ")([^"]+)(")/m

function validateVersion(version) {
  const match = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$/.exec(version)
  if (!match || match[0] !== version || match.slice(1, 4).some(part => Number(part) > 65535)) {
    throw new Error('版本号须为 X.Y.Z 或 X.Y.Z-rc.1，各数字不超过 65535，不支持构建元数据。')
  }
  if (match[4]?.split('.').some(part => /^\d+$/.test(part) && part.length > 1 && part.startsWith('0'))) {
    throw new Error('预发布版本的纯数字标识不能有前导零。')
  }
  return version
}

function readVersions() {
  return [
    ['package.json', 'json'],
    ['src-tauri/tauri.conf.json', 'json'],
    ['src-tauri/Cargo.toml', cargoVersion],
    ['src-tauri/Cargo.lock', lockVersion],
  ].map(([path, format]) => {
    const text = readFileSync(resolve(root, path), 'utf8')
    const version = format === 'json' ? JSON.parse(text).version : text.match(format)?.[2]
    if (typeof version !== 'string') throw new Error(`${path} 中未找到应用版本号。`)
    validateVersion(version)
    return { path, format, text, version }
  })
}

function checkVersions(files, tag) {
  const version = files[0].version
  if (files.some(file => file.version !== version)) {
    throw new Error(`应用版本号不一致：\n${files.map(file => `${file.path}: ${file.version}`).join('\n')}\n请先运行 pnpm release:version <版本号>。`)
  }
  if (tag !== undefined) {
    if (!tag.startsWith('v')) throw new Error('发布标签必须使用 v 前缀，例如 v0.2.0。')
    validateVersion(tag.slice(1))
    if (tag !== `v${version}`) throw new Error(`发布标签 ${tag} 与应用版本 v${version} 不一致。`)
  }
  return { version, tag: `v${version}`, prerelease: version.includes('-') }
}

try {
  const [command, argument, ...extra] = process.argv.slice(2)
  if (extra.length || !['set', 'check'].includes(command) || (command === 'set' && !argument)) {
    throw new Error('用法：pnpm release:version <版本号> 或 pnpm release:check [v版本号]')
  }
  const files = readVersions()
  if (command === 'set') {
    const version = validateVersion(argument)
    // 先读取并校验所有文件，再只替换应用自身的版本；不刷新依赖锁定结果。
    const updates = files.map(file => ({
      ...file,
      next: file.format === 'json'
        ? `${JSON.stringify({ ...JSON.parse(file.text), version }, null, 2)}\n`
        : file.text.replace(file.format, (_match, prefix, _old, suffix) => `${prefix}${version}${suffix}`),
    }))
    for (const file of updates) {
      if (file.next !== file.text) writeFileSync(resolve(root, file.path), file.next)
    }
    console.log(`应用版本已同步为 ${version}；发布标签为 v${version}。`)
  } else {
    console.log(JSON.stringify(checkVersions(files, argument)))
  }
} catch (error) {
  console.error(error.message)
  process.exitCode = 1
}
