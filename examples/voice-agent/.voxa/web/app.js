'use strict'

const tokenKey = 'voxa.voice.token'
const fragment = location.hash.slice(1)
if (fragment) sessionStorage.setItem(tokenKey, fragment)
history.replaceState(null, '', location.pathname)
const token = fragment || sessionStorage.getItem(tokenKey) || ''
const headers = () => ({ Authorization: `Bearer ${token}` })
const $ = (selector) => document.querySelector(selector)
let client = null
let microphone = null
let meterTimer = null
let runtimeTimer = null
let lastEventSignature = ''

for (let index = 0; index < 32; index += 1) {
  const level = document.createElement('i')
  $('#levels').append(level)
}

async function api(path, options = {}) {
  const response = await fetch(path, { ...options, headers: { ...headers(), ...(options.headers || {}) } })
  const text = await response.text()
  let body = text
  try { body = JSON.parse(text) } catch (_) {}
  if (!response.ok) throw new Error(body?.message || body || response.statusText)
  return body
}

function message(text, detail = '') {
  $('#voice-state').textContent = text
  if (detail) $('#session-copy').textContent = detail
}

function showError(error) {
  $('#error').hidden = false
  $('#error').textContent = `${error.message}\n\nOpen Studio → Connections and verify the three RTC identities plus DashScope credentials. Build the C++ Node Packs with the real Agora SDK before launching.`
}

async function startRuntime() {
  const graph = await api('/api/v1/graph')
  const status = await api('/api/v1/runtime')
  if (status.status !== 'running') {
    await api('/api/v1/runtime/start', {
      method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(graph),
    })
  }
}

async function join() {
  $('#launch').disabled = true
  $('#error').hidden = true
  try {
    if (!token) throw new Error('Studio access token is missing. Open Voice Room from Studio.')
    if (!window.AgoraRTC) throw new Error('Agora Web SDK could not be loaded.')
    const connection = (await api('/api/v1/connections/client')).agora || {}
    for (const field of ['app_id', 'channel', 'web_uid', 'web_token']) {
      if (!connection[field]) throw new Error(`Agora browser field ${field} is not configured.`)
    }
    message('Starting Voxa graph…', 'Loading native and Python Node Packs')
    await startRuntime()
    client = window.AgoraRTC.createClient({ mode: 'rtc', codec: 'vp8' })
    client.on('user-published', async (user, mediaType) => {
      await client.subscribe(user, mediaType)
      if (mediaType === 'audio') {
        user.audioTrack.play()
        $('#orb').classList.add('speaking')
        message('Voxa is speaking', 'Interrupt naturally — the VAD control plane will cancel stale output')
        setTimeout(() => $('#orb').classList.remove('speaking'), 900)
      }
    })
    client.on('user-unpublished', () => $('#orb').classList.remove('speaking'))
    await client.join(connection.app_id, connection.channel, connection.web_token, Number(connection.web_uid))
    microphone = await window.AgoraRTC.createMicrophoneAudioTrack({ encoderConfig: 'speech_standard' })
    await client.publish([microphone])
    $('#orb').classList.add('live')
    $('#orb span').textContent = 'LIVE'
    $('#launch').hidden = true
    $('#leave').hidden = false
    message('Listening — say something', 'This session stays open. You can speak over the assistant at any time.')
    startMeter()
    pollRuntime()
  } catch (error) {
    $('#launch').disabled = false
    showError(error)
  }
}

function startMeter() {
  clearInterval(meterTimer)
  meterTimer = setInterval(() => {
    const volume = microphone?.getVolumeLevel?.() || 0
    const bars = [...document.querySelectorAll('#levels i')]
    bars.forEach((bar, index) => {
      const curve = Math.sin((index / bars.length) * Math.PI)
      const jitter = .35 + Math.random() * .65
      bar.style.height = `${5 + Math.max(volume * 90 * curve * jitter, 1)}px`
    })
  }, 80)
}

async function pollRuntime() {
  clearTimeout(runtimeTimer)
  try {
    const runtime = await api('/api/v1/runtime')
    const live = runtime.status === 'running'
    $('#runtime-pill').classList.toggle('live', live)
    $('#runtime-pill b').textContent = live ? 'Runtime live' : runtime.status
    $('#graph-name').textContent = runtime.graph_id || '—'
    $('#calls').textContent = (runtime.nodes || []).reduce((sum, node) => sum + (node.callback_total || 0), 0)
    $('#frames').textContent = (runtime.edges || []).reduce((sum, edge) => sum + (edge.enqueue_total || 0), 0)
    for (const stage of document.querySelectorAll('.pipeline div')) {
      const hint = stage.dataset.node
      stage.classList.toggle('active', live && (runtime.nodes || []).some(node => node.node_id.includes(hint) && node.callback_total > 0))
    }
    if (runtime.terminal?.error) showError(new Error(runtime.terminal.error))
    await renderVoiceEvents()
  } catch (error) { showError(error) }
  runtimeTimer = setTimeout(pollRuntime, 700)
}

async function renderVoiceEvents() {
  const events = await api('/api/v1/runtime/events')
  let start = 0
  if (lastEventSignature) {
    const found = events.findIndex(event => JSON.stringify(event) === lastEventSignature)
    if (found >= 0) start = found + 1
  }
  for (const event of events.slice(start)) {
    const text = typeof event.payload?.text === 'string' ? event.payload.text : ''
    if (event.topic === 'voxa.voice.speech.started') {
      $('#user-text').textContent = ''
      $('#agent-text').textContent = ''
      message('Listening — speak naturally', 'Barge-in signal sent; stale output is being cancelled')
    } else if (event.topic === 'voxa.voice.transcript.delta') {
      $('#user-text').textContent += text
    } else if (event.topic === 'voxa.voice.transcript.completed') {
      $('#user-text').textContent = text
      message('Thinking…', 'Transcript committed to the typed Graph')
    } else if (event.topic === 'voxa.voice.response.delta') {
      $('#agent-text').textContent += text
      message('Voxa is responding', 'Text and audio are streaming through separate typed branches')
    }
  }
  if (events.length) lastEventSignature = JSON.stringify(events[events.length - 1])
}

async function leave() {
  clearInterval(meterTimer)
  clearTimeout(runtimeTimer)
  if (microphone) { microphone.stop(); microphone.close(); microphone = null }
  if (client) { await client.leave(); client = null }
  try { await api('/api/v1/runtime/stop', { method: 'POST' }) } catch (_) {}
  $('#orb').className = 'orb'
  $('#orb span').textContent = 'READY'
  lastEventSignature = ''
  $('#launch').hidden = false
  $('#launch').disabled = false
  $('#leave').hidden = true
  message('Session ended', 'Start again whenever you are ready.')
  await pollRuntime()
}

$('#launch').addEventListener('click', join)
$('#leave').addEventListener('click', leave)
window.addEventListener('beforeunload', () => { microphone?.close(); client?.leave() })
pollRuntime()
