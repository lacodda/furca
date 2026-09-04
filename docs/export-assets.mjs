// Rasterizes the furca brand SVGs into PNGs, a multi-size .ico, and the icon
// set Tauri bundles. Masters live in the vault brand registry; the SVGs under
// assets/ and docs/ are copies of them and are never edited by hand.
//
// Run from docs/ so the ESM import resolves the local sharp install:
//   cd docs && pnpm exec node export-assets.mjs
import sharp from 'sharp'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const DOCS = path.dirname(fileURLToPath(import.meta.url))
const ROOT = path.resolve(DOCS, '..')
const ASSETS = path.join(ROOT, 'assets')
const TAURI_ICONS = path.join(ROOT, 'src-tauri', 'icons')

// The three levels of the mark. Which one a raster takes is decided by
// `levelFor` below, never by habit: L and M carry the near-black plate with a
// gradient outline and code, S is the plate filled solid with the gradient
// and dark text — the only one of the three that still reads as a colour
// block once it is a handful of pixels.
const S = path.join(ASSETS, 'logo-s.svg')
const M = path.join(ASSETS, 'logo-m.svg')
const L = path.join(ASSETS, 'logo.svg')
const BANNER = path.join(ASSETS, 'banner.svg')

// Which level of the mark a given icon size carries. Below 32px only the
// filled S tile survives as a colour block; the plated mark needs room for
// its outline and code, which M has at 32-48px and L has from 64px up, where
// the "commit graph" trace beneath the code also stays legible.
function levelFor(size) {
  if (size <= 24) return S
  if (size <= 48) return M
  return L
}

// Largest first. Windows picks by *closest size* and ignores order (see
// "About Icons", Icon Display), but tauri-codegen takes `entries()[0]`
// verbatim as the window icon — so the first entry is the one the titlebar
// stretches from, and a small first entry reads smeared.
const ICO_SIZES = [256, 128, 64, 48, 32, 24, 16]
// Sizes Tauri's bundler expects on Windows, macOS and Linux.
const TAURI_PNG = [32, 128, 256, 512]

async function png(src, size, out) {
  await sharp(src, { density: 384 }).resize(size, size).png().toFile(out)
}

// Minimal ICO container: header + directory entries + embedded PNG payloads.
// sharp cannot write this container itself.
function buildIco(pngBuffers, sizes) {
  const count = pngBuffers.length
  const header = Buffer.alloc(6)
  header.writeUInt16LE(0, 0) // reserved
  header.writeUInt16LE(1, 2) // type: icon
  header.writeUInt16LE(count, 4)

  const entries = Buffer.alloc(16 * count)
  let offset = 6 + 16 * count
  pngBuffers.forEach((buf, i) => {
    const size = sizes[i]
    const e = 16 * i
    entries.writeUInt8(size >= 256 ? 0 : size, e + 0) // width (0 means 256)
    entries.writeUInt8(size >= 256 ? 0 : size, e + 1) // height
    entries.writeUInt8(0, e + 2) // palette
    entries.writeUInt8(0, e + 3) // reserved
    entries.writeUInt16LE(1, e + 4) // colour planes
    entries.writeUInt16LE(32, e + 6) // bits per pixel
    entries.writeUInt32LE(buf.length, e + 8)
    entries.writeUInt32LE(offset, e + 12)
    offset += buf.length
  })

  return Buffer.concat([header, entries, ...pngBuffers])
}

fs.mkdirSync(TAURI_ICONS, { recursive: true })

const icoParts = []
for (const size of ICO_SIZES) {
  icoParts.push(await sharp(levelFor(size), { density: 384 }).resize(size, size).png().toBuffer())
}
const ico = buildIco(icoParts, ICO_SIZES)
fs.writeFileSync(path.join(ASSETS, 'icon.ico'), ico)
console.log('wrote icon.ico')

// Favicon + docs touch icon.
await png(S, 32, path.join(ASSETS, 'favicon-32.png'))
// 180px is a home-screen tile, well past the 64px line where the full mark
// has room for its outline and the graph trace beneath the code.
await png(levelFor(180), 180, path.join(ASSETS, 'apple-touch-icon.png'))
await png(L, 512, path.join(ASSETS, 'logo-512.png'))
console.log('wrote pngs')

// docs/public gets its own copy: Starlight's head config points an absolute
// `/apple-touch-icon.png` link at it, and the public dir cannot symlink into
// assets/ across a static build.
fs.copyFileSync(path.join(ASSETS, 'apple-touch-icon.png'), path.join(ROOT, 'docs', 'public', 'apple-touch-icon.png'))

// The application's own icons — generated from the same masters as the
// brand, so the two cannot drift. Every size carries the level that reads at
// it, same as the brand icon.ico built above.
fs.writeFileSync(path.join(TAURI_ICONS, 'icon.ico'), ico)
await png(L, 512, path.join(TAURI_ICONS, 'icon.png'))
// Sizes the Tauri template ships with. Nothing in tauri.conf.json names them
// directly (the bundle icon list points at icon.png/icon.ico), but they are
// in the repo, and a stale copy of the mark is worse than none.
await png(levelFor(32), 32, path.join(TAURI_ICONS, '32x32.png'))
await png(levelFor(128), 128, path.join(TAURI_ICONS, '128x128.png'))
await png(levelFor(256), 256, path.join(TAURI_ICONS, '256x256.png'))
console.log('wrote application icons')
console.log(
  'icon.icns is not written here — sharp cannot emit it. ' +
    'Feed the 1024px L raster to `pnpm tauri icon` (or however the platform tool at hand builds one) ' +
    'and keep only the resulting src-tauri/icons/icon.icns.',
)

// GitHub social preview: 1280x640 on the line's plate colour. The banner's
// plate spans the full 720px while the artwork fills the left ~570px, so the
// tail is trimmed; the rounded plate over an identical background would
// leave a visible seam, so only the inner rows are kept.
const bannerWidth = 1600
const bannerHeight = Math.round((bannerWidth * 170) / 720)
const inset = Math.round((bannerWidth * 6) / 720) // clears the plate's rounded edge
const banner = await sharp(BANNER, { density: 384 })
  .resize({ width: bannerWidth })
  .extract({ left: inset, top: inset, width: 1290 - inset, height: bannerHeight - 2 * inset })
  .png()
  .toBuffer()

await sharp({
  create: { width: 1280, height: 640, channels: 4, background: '#1B2126' },
})
  .composite([{ input: await sharp(banner).resize({ width: 880 }).png().toBuffer(), gravity: 'centre' }])
  .png()
  .toFile(path.join(ASSETS, 'social-preview.png'))
console.log('wrote social-preview.png')
