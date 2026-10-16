<script setup lang="ts">
// "About": what this is, which version, under which license and where the source and the license files are. Everything comes from the server (the crate manifest),
// so that a fork that changes the license or the repository changes it in one place.
import { computed } from 'vue'
import { BookOpen, Bug, Code2, FileText, Scale } from 'lucide-vue-next'
import BrandLogo from '@/components/BrandLogo.vue'
import Modal from '@/components/Modal.vue'
import { t } from '@/i18n'
import { safeHttpUrl, useApp } from '@/stores/app'

const emit = defineEmits<{ close: [] }>()
const app = useApp()

const about = computed(() => app.boot?.about)
const repo = computed(() => safeHttpUrl(about.value?.repository))
/** A file of the repository, on its main branch. */
const file = (name: string) => (repo.value ? `${repo.value}/blob/main/${name}` : '')
const docs = computed(() => app.helpUrl || (repo.value ? `${repo.value}/tree/main/docs` : ''))
const links = computed(() => [
  { id: 'license', icon: Scale, label: t('about.license'), text: about.value?.license ?? '', hint: t('about.license.hint'), href: file('LICENSE') },
  { id: 'third', icon: FileText, label: t('about.third'), text: 'THIRD_PARTY_LICENSES.md', hint: t('about.third.hint'), href: file('THIRD_PARTY_LICENSES.md') },
  { id: 'source', icon: Code2, label: t('about.source'), text: repo.value.replace(/^https?:\/\//, ''), hint: '', href: repo.value },
  { id: 'docs', icon: BookOpen, label: t('help.docs'), text: '', hint: '', href: docs.value },
  { id: 'issues', icon: Bug, label: t('about.issues'), text: '', hint: '', href: repo.value ? `${repo.value}/issues` : '' },
].filter((l) => l.href))
</script>

<template>
  <Modal :title="t('about.title')" @close="emit('close')">
    <div class="about" data-testid="about">
      <header>
        <BrandLogo :size="46" />
        <div class="who">
          <h3>{{ app.appName }}</h3>
          <p class="ver mono"><span data-testid="about-version">{{ t('about.version', { v: app.boot?.version ?? '' }) }}</span> · {{ t('about.engine', { v: app.boot?.engineVersion ?? '' }) }}</p>
        </div>
      </header>
      <p class="lead">{{ t('about.lead') }}</p>
      <ul>
        <li v-for="l in links" :key="l.id">
          <a :href="l.href" target="_blank" rel="noopener noreferrer" :data-testid="'about-' + l.id">
            <component :is="l.icon" :size="16" class="ic" />
            <span class="txt"><b>{{ l.label }}<template v-if="l.text"> · <span class="mono val">{{ l.text }}</span></template></b><small v-if="l.hint">{{ l.hint }}</small></span>
          </a>
        </li>
      </ul>
      <p class="notes faint">{{ t('about.notes') }}</p>
    </div>
  </Modal>
</template>

<style scoped>
.about { display: flex; flex-direction: column; gap: 14px; }
header { display: flex; align-items: center; gap: 14px; }
h3 { margin: 0; font-size: 18px; font-weight: 650; letter-spacing: -.01em; }
.ver { margin: 2px 0 0; font-size: 12.5px; color: var(--muted); }
.lead { margin: 0; color: var(--text-2); line-height: 1.5; }
ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; border: 1px solid var(--line); border-radius: var(--radius); overflow: hidden; }
li + li { border-top: 1px solid var(--line); }
a { display: flex; align-items: flex-start; gap: 12px; padding: 10px 12px; color: var(--text); text-decoration: none; }
a:hover { background: var(--surface-sunken); text-decoration: none; }
a:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
.ic { color: var(--muted); flex: none; margin-top: 2px; }
.txt { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
.txt b { font-size: 13.5px; font-weight: 600; overflow-wrap: anywhere; }
.val { font-weight: 500; color: var(--accent); }
.txt small { font-size: 12.5px; color: var(--muted); line-height: 1.4; }
.notes { margin: 0; font-size: 12px; line-height: 1.5; }
</style>
