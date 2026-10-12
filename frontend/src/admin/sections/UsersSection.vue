<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { KeyRound, Lock, Plus, RefreshCw } from 'lucide-vue-next'
import * as api from '@/api/endpoints'
import type { AdminUser, Role } from '@/api/types'
import Modal from '@/components/Modal.vue'
import { t } from '@/i18n'
import { errorMessage, fmtDateTime, fmtRelative } from '@/lib/format'
import { useApp } from '@/stores/app'
import { useToasts } from '@/stores/toasts'
import StateBlock from '../components/StateBlock.vue'
import TempPasswordModal from '../components/TempPasswordModal.vue'
import { activeAdmins, isLocked, protection } from '../lib/users'

const app = useApp()
const toasts = useToasts()
const ROLES: Role[] = ['viewer', 'editor', 'admin']

const users = ref<AdminUser[]>([])
const loading = ref(false)
const error = ref<string | null>(null)
const busy = ref<Set<string>>(new Set())

const creating = ref(false)
const form = ref({ email: '', name: '', role: 'editor' as Role, password: '' })
const formError = ref<string | null>(null)
const submitting = ref(false)

const resetting = ref<AdminUser | null>(null)
const resetPassword = ref('')
const resetError = ref<string | null>(null)

const temp = ref<{ email: string; password: string; created: boolean } | null>(null)

const admins = computed(() => activeAdmins(users.value))

async function load(spinner = true) {
  if (spinner) loading.value = true
  try {
    users.value = (await api.admin.users()).users
    error.value = null
  } catch (e) {
    error.value = errorMessage(e)
  } finally {
    loading.value = false
  }
}
onMounted(() => load())

function prot(u: AdminUser) {
  return protection(u, app.user?.id, admins.value)
}

async function patch(u: AdminUser, body: { name?: string; role?: string; disabled?: boolean }, el?: HTMLInputElement | HTMLSelectElement, revert?: string) {
  busy.value = new Set(busy.value).add(u.id)
  try {
    await api.admin.patchUser(u.id, body)
    toasts.ok(t('admin.users.updated', { email: u.email }))
  } catch (e) {
    toasts.error(errorMessage(e))
    if (el && revert !== undefined) el.value = revert
  } finally {
    const n = new Set(busy.value)
    n.delete(u.id)
    busy.value = n
    await load(false)
  }
}

function onRole(u: AdminUser, e: Event) {
  const el = e.target as HTMLSelectElement
  if (el.value === u.role) return
  return patch(u, { role: el.value }, el, u.role)
}

function onName(u: AdminUser, e: Event) {
  const el = e.target as HTMLInputElement
  const v = el.value.trim()
  if (v === u.name) return
  return patch(u, { name: v }, el, u.name)
}

function openCreate() {
  form.value = { email: '', name: '', role: 'editor', password: '' }
  formError.value = null
  creating.value = true
}

async function create() {
  submitting.value = true
  formError.value = null
  try {
    const r = await api.admin.createUser({ email: form.value.email.trim(), name: form.value.name.trim(), role: form.value.role, password: form.value.password || undefined })
    creating.value = false
    if (r.temporaryPassword) temp.value = { email: form.value.email.trim().toLowerCase(), password: r.temporaryPassword, created: true }
    else toasts.ok(t('admin.users.created', { email: form.value.email.trim() }))
    await load(false)
  } catch (e) {
    formError.value = errorMessage(e)
  } finally {
    submitting.value = false
  }
}

function openReset(u: AdminUser) {
  resetting.value = u
  resetPassword.value = ''
  resetError.value = null
}

async function doReset() {
  const u = resetting.value
  if (!u) return
  submitting.value = true
  resetError.value = null
  try {
    const r = await api.admin.resetPassword(u.id, resetPassword.value || undefined)
    resetting.value = null
    if (r.temporaryPassword) temp.value = { email: u.email, password: r.temporaryPassword, created: false }
    else toasts.ok(t('admin.users.passwordSet', { email: u.email }))
    await load(false)
  } catch (e) {
    resetError.value = errorMessage(e)
  } finally {
    submitting.value = false
  }
}
</script>

<template>
  <section class="adm-page">
    <header class="adm-head">
      <div class="grow">
        <h1>{{ t('admin.users.title') }}</h1>
        <p class="muted">{{ t('admin.users.subtitle') }}</p>
      </div>
      <button class="btn btn-icon" :title="t('common.refresh')" :aria-label="t('common.refresh')" :disabled="loading" @click="load()"><RefreshCw :size="15" /></button>
      <button class="btn btn-primary" @click="openCreate"><Plus :size="15" />{{ t('admin.users.create') }}</button>
    </header>

    <StateBlock :loading="loading && !users.length" :error="error" :empty="!users.length" :empty-text="t('admin.users.empty')" @retry="load()">
      <div class="card tablewrap">
        <table class="table users-table">
          <thead>
            <tr>
              <th>{{ t('admin.users.colEmail') }}</th><th>{{ t('admin.users.colName') }}</th><th>{{ t('admin.users.colRole') }}</th><th>{{ t('admin.users.colActive') }}</th>
              <th>{{ t('admin.users.colLogin') }}</th><th>{{ t('admin.users.colStatus') }}</th><th class="actions-col"><span class="sr-only">{{ t('admin.items.colActions') }}</span></th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="u in users" :key="u.id" :class="{ disabled: u.disabled, busy: busy.has(u.id) }">
              <td>
                <div class="truncate">{{ u.email }}</div>
                <div v-if="u.id === app.user?.id" class="faint">{{ t('admin.users.you') }}</div>
              </td>
              <td><input class="input input-sm name-input" :value="u.name" :aria-label="t('admin.users.nameOf', { email: u.email })" :disabled="busy.has(u.id)" maxlength="120" @change="onName(u, $event)" /></td>
              <td>
                <select class="select input-sm role-select" :value="u.role" :disabled="busy.has(u.id) || !!prot(u)" :title="prot(u) ? t(`admin.users.protect.${prot(u)}`) : ''" :aria-label="t('admin.users.roleOf', { email: u.email })" @change="onRole(u, $event)">
                  <option v-for="r in ROLES" :key="r" :value="r">{{ t(`admin.users.role.${r}`) }}</option>
                </select>
              </td>
              <td>
                <button
                  type="button" class="switch" role="switch" :aria-checked="!u.disabled" :disabled="busy.has(u.id) || !!prot(u)" :title="prot(u) ? t(`admin.users.protect.${prot(u)}`) : ''"
                  :aria-label="t('admin.users.activeOf', { email: u.email })" @click="patch(u, { disabled: !u.disabled })"
                ><span class="switch-knob" /></button>
              </td>
              <td class="nowrap" :title="u.lastLoginAt ? fmtDateTime(u.lastLoginAt) : ''">{{ u.lastLoginAt ? fmtRelative(u.lastLoginAt) : t('admin.users.never') }}</td>
              <td>
                <div class="badges">
                  <span v-if="isLocked(u)" class="tag tag-crit" :title="t('admin.users.unlockHint')"><Lock :size="11" />{{ t('admin.users.lockedUntil', { when: fmtDateTime(u.lockedUntil) }) }}</span>
                  <span v-if="u.mustChangePassword" class="tag tag-risk">{{ t('admin.users.mustChange') }}</span>
                  <span v-if="u.disabled" class="tag tag-nodata">{{ t('admin.users.disabled') }}</span>
                </div>
                <div v-if="isLocked(u)" class="faint hint-line">{{ t('admin.users.unlockHint') }}</div>
              </td>
              <td class="actions"><button class="btn btn-sm" @click="openReset(u)"><KeyRound :size="13" />{{ t('admin.users.reset') }}</button></td>
            </tr>
          </tbody>
        </table>
      </div>
    </StateBlock>

    <Modal v-if="creating" :title="t('admin.users.createTitle')" :closable="!submitting" @close="creating = false">
      <form class="col" style="gap: 12px" @submit.prevent="create">
        <div class="field"><label for="nu-email">{{ t('admin.users.colEmail') }}</label><input id="nu-email" v-model="form.email" class="input" type="email" required autocomplete="off" /></div>
        <div class="field"><label for="nu-name">{{ t('admin.users.colName') }}</label><input id="nu-name" v-model="form.name" class="input" autocomplete="off" maxlength="120" /></div>
        <div class="field">
          <label for="nu-role">{{ t('admin.users.colRole') }}</label>
          <select id="nu-role" v-model="form.role" class="select"><option v-for="r in ROLES" :key="r" :value="r">{{ t(`admin.users.role.${r}`) }}</option></select>
          <span class="hint">{{ t(`admin.users.roleHint.${form.role}`) }}</span>
        </div>
        <div class="field">
          <label for="nu-pass">{{ t('admin.users.password') }}</label>
          <input id="nu-pass" v-model="form.password" class="input mono" type="password" autocomplete="new-password" />
          <span class="hint">{{ t('admin.users.passwordHint') }}</span>
        </div>
        <div v-if="formError" class="banner banner-err" role="alert">{{ formError }}</div>
        <button type="submit" class="sr-only" tabindex="-1">{{ t('common.create') }}</button>
      </form>
      <template #footer>
        <button class="btn" :disabled="submitting" @click="creating = false">{{ t('common.cancel') }}</button>
        <button class="btn btn-primary" :disabled="submitting || !form.email.trim()" @click="create"><span v-if="submitting" class="spinner" />{{ t('common.create') }}</button>
      </template>
    </Modal>

    <Modal v-if="resetting" :title="t('admin.users.resetTitle', { email: resetting.email })" :closable="!submitting" @close="resetting = null">
      <form class="col" style="gap: 12px" @submit.prevent="doReset">
        <p style="margin: 0">{{ t('admin.users.resetText') }}</p>
        <div class="field">
          <label for="rp-pass">{{ t('admin.users.password') }}</label>
          <input id="rp-pass" v-model="resetPassword" class="input mono" type="password" autocomplete="new-password" />
          <span class="hint">{{ t('admin.users.passwordHint') }}</span>
        </div>
        <div v-if="resetError" class="banner banner-err" role="alert">{{ resetError }}</div>
        <button type="submit" class="sr-only" tabindex="-1">{{ t('admin.users.reset') }}</button>
      </form>
      <template #footer>
        <button class="btn" :disabled="submitting" @click="resetting = null">{{ t('common.cancel') }}</button>
        <button class="btn btn-danger" :disabled="submitting" @click="doReset"><span v-if="submitting" class="spinner" />{{ t('admin.users.resetDo') }}</button>
      </template>
    </Modal>

    <TempPasswordModal v-if="temp" :email="temp.email" :password="temp.password" :created="temp.created" @close="temp = null" />
  </section>
</template>
