<!-- The TrailMark mascot: a bookmark that keeps your place, eyes that follow changes, and a highlighted line
     for a mouth. Same drawing as docs/assets/logo.svg. Decorative: the app name is always next to it. -->
<script lang="ts">
  let { size = 64 }: { size?: number } = $props();
</script>

<svg class="brand-mark" viewBox="0 0 256 256" width={size} height={size} aria-hidden="true" focusable="false">
  <path d="M70 22h116a22 22 0 0 1 22 22v190l-80-50-80 50V44a22 22 0 0 1 22-22z" fill="#f2542d" />
  <circle cx="104" cy="92" r="21" fill="#fff" />
  <circle cx="108" cy="97" r="11" fill="#1b1520" />
  <circle cx="112" cy="93" r="3.4" fill="#fff" />
  <circle cx="152" cy="92" r="21" fill="#fff" />
  <circle cx="156" cy="97" r="11" fill="#1b1520" />
  <circle cx="160" cy="93" r="3.4" fill="#fff" />
  <rect x="96" y="128" width="64" height="20" rx="10" fill="#ffe066" />
</svg>
