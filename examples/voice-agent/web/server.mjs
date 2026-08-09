import { createReadStream, statSync } from 'node:fs'
import { createServer } from 'node:http'
import { dirname, extname, join, normalize, sep } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = dirname(fileURLToPath(import.meta.url))
const arguments_ = process.argv.slice(2)

function option(name, fallback) {
  const index = arguments_.indexOf(name)
  return index >= 0 ? arguments_[index + 1] : fallback
}

const host = option('--host', '127.0.0.1')
const port = Number(option('--port', '4173'))
if (!Number.isInteger(port) || port < 1 || port > 65535) {
  console.error('[MUXIVA][VOICE-ROOM][ERROR] --port must be between 1 and 65535')
  process.exit(2)
}

const contentTypes = {
  '.css': 'text/css; charset=utf-8',
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.png': 'image/png',
  '.svg': 'image/svg+xml',
}

createServer((request, response) => {
  try {
    const pathname = decodeURIComponent(new URL(request.url, 'http://localhost').pathname)
    const relative = pathname === '/' ? 'index.html' : pathname.replace(/^\/+/, '')
    const file = normalize(join(root, relative))
    if (!file.startsWith(`${root}${sep}`) || !statSync(file).isFile()) throw new Error('not found')
    response.writeHead(200, {
      'Content-Type': contentTypes[extname(file)] || 'application/octet-stream',
      'Cache-Control': 'no-store',
      'X-Content-Type-Options': 'nosniff',
    })
    createReadStream(file).pipe(response)
  } catch (_) {
    response.writeHead(404, { 'Content-Type': 'text/plain; charset=utf-8' })
    response.end('Not found')
  }
}).listen(port, host, () => {
  console.log(`[MUXIVA][VOICE-ROOM][READY] url=http://${host}:${port}`)
  console.log('[MUXIVA][VOICE-ROOM][INFO] This process serves browser files only; it does not run a Graph or hold model credentials.')
})
