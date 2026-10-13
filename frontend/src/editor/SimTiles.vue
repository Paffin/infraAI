<script setup lang="ts">
// The four figures of a failure as tiles: recovery, data loss, capacity, nodes hit. They follow the picture on screen where it matters
// (capacity and the nodes change while the recovery runs); how they are worked out is lib/simTiles.ts.
import type { Tile } from '@/lib/simTiles'

defineProps<{ tiles: Tile[] }>()
</script>

<template>
  <div class="tiles" data-testid="sim-tiles">
    <div v-for="x in tiles" :key="x.id" class="tile" :class="x.tone" :data-tile="x.id">
      <span class="l">{{ x.label }}</span>
      <Transition name="val" mode="out-in"><b :key="x.value" class="v">{{ x.value }}</b></Transition>
      <span v-if="x.hint" class="h">{{ x.hint }}</span>
    </div>
  </div>
</template>

<style scoped>
.tiles { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; margin: 10px 0 4px; }
.tile { display: flex; flex-direction: column; gap: 2px; min-width: 0; padding: 8px 10px 9px; border-radius: var(--radius); background: var(--surface-sunken); border: 1px solid transparent; transition: background-color var(--dur-base), border-color var(--dur-base), color var(--dur-base); }
.tile.ok { background: var(--ok-bg); }
.tile.risk { background: var(--risk-bg); }
.tile.crit { background: var(--crit-bg); }
.l { font-size: 10.5px; letter-spacing: .06em; text-transform: uppercase; color: var(--label); font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.v { font-size: 17px; line-height: 1.2; font-weight: 700; letter-spacing: -.01em; color: var(--text); font-variant-numeric: tabular-nums; overflow-wrap: anywhere; }
.ok .v { color: var(--ok); } .risk .v { color: var(--risk); } .crit .v { color: var(--crit); }
.h { font-size: 11.5px; line-height: 1.35; color: var(--muted); overflow-wrap: anywhere; }
.val-enter-active { transition: opacity var(--dur-base) var(--ease-out), transform var(--dur-base) var(--ease-out); }
.val-leave-active { transition: opacity var(--dur-instant) var(--ease-out); }
.val-enter-from { opacity: 0; transform: translateY(calc(var(--move) * .6)); }
.val-leave-to { opacity: 0; }
</style>
