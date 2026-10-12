import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import * as api from '@/api/endpoints'
import type { ConfigError, ConfigKindCounter as KindCounter } from '@/api/types'
import { errorMessage } from '@/lib/format'

export type { ConfigError, ConfigKindCounter as KindCounter } from '@/api/types'

/** Per-kind counters and the items that failed to parse: shown in the sub-navigation and as a banner. */
export const useAdminConfig = defineStore('adminConfig', () => {
  const kinds = ref<KindCounter[]>([])
  const errors = ref<ConfigError[]>([])
  const loading = ref(false)
  const error = ref<string | null>(null)
  const loaded = ref(false)

  async function load() {
    loading.value = true
    try {
      const r = await api.admin.configKinds()
      kinds.value = r.kinds
      errors.value = r.errors
      error.value = null
      loaded.value = true
    } catch (e) {
      error.value = errorMessage(e)
    } finally {
      loading.value = false
    }
  }

  function counter(kind: string): KindCounter | undefined {
    return kinds.value.find((k) => k.kind === kind)
  }

  const errorCount = computed(() => errors.value.length)
  return { kinds, errors, loading, error, loaded, load, counter, errorCount }
})
