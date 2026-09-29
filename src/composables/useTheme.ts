import { ref, onMounted } from 'vue'
import { themes, applyTheme, type ThemeDefinition } from '@/themes'

const currentTheme = ref<string>('dark')
const themeList = themes

export function useTheme() {
  onMounted(() => {
    const saved = localStorage.getItem('theme')
    applyTheme(saved || 'dark')
    if (saved) currentTheme.value = saved
  })

  function setTheme(name: string) {
    applyTheme(name)
    currentTheme.value = name
  }

  return { currentTheme, themeList, setTheme }
}
