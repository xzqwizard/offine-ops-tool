import { light } from './definitions/light'
import { dark } from './definitions/dark'
import { techGreen } from './definitions/tech-green'
import { businessBlue } from './definitions/business-blue'
import { auroraPurple } from './definitions/aurora-purple'

export interface ThemeDefinition {
  name: string
  label: string
  /** 是否为深色系（决定 Element Plus 的 dark class） */
  dark: boolean
  vars: Record<string, string>
}

export const themes: ThemeDefinition[] = [dark, light, techGreen, businessBlue, auroraPurple]

export function applyTheme(name: string) {
  const theme = themes.find((t) => t.name === name)
  if (!theme) return
  const root = document.documentElement
  root.setAttribute('data-theme', theme.name)
  root.classList.toggle('dark', theme.dark)
  for (const [key, value] of Object.entries(theme.vars)) {
    root.style.setProperty(key, value)
  }
  localStorage.setItem('theme', name)
}

export function loadSavedTheme() {
  const saved = localStorage.getItem('theme') || 'dark'
  applyTheme(saved)
}
