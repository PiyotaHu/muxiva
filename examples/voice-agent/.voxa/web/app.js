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
let sessionStartedAt = 0
let lastPipelineState = ''
let lastErrorMessage = ''

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

function diagnostic(text, error = false) {
  const item = document.createElement('li')
  if (error) item.className = 'error-line'
  const time = document.createElement('time')
  time.textContent = new Date().toLocaleTimeString()
  item.append(time, document.createTextNode(text))
  const log = $('#diagnostic-log')
  log.append(item)
  while (log.children.length > 100) log.firstElementChild.remove()
  log.scrollTop = log.scrollHeight
}

function showError(error) {
  $('#error').hidden = false
  $('#error').textContent = `${error.message}\n\nOpen Studio → Connections and verify the browser and Voxa Bot RTC identities plus DashScope credentials. Runtime details are saved in .voxa/runtime.log.`
  if (error.message !== lastErrorMessage) {
    diagnostic(`ERROR · ${error.message}`, true)
    lastErrorMessage = error.message
  }
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
    lastErrorMessage = ''
    $('#diagnostic-log').replaceChildren()
    lastPipelineState = ''
  try {
    if (!token) throw new Error('Studio access token is missing. Open Voice Room from Studio.')
    if (!window.AgoraRTC) throw new Error('Agora Web SDK could not be loaded.')
    const connection = (await api('/api/v1/connections/client')).agora || {}
    for (const field of ['app_id', 'channel', 'web_uid', 'web_token']) {
      if (!connection[field]) throw new Error(`Agora browser field ${field} is not configured.`)
    }
    message('Starting Voxa graph…', 'Loading native and Python Node Packs')
    diagnostic('Starting Voxa Runtime and loading Provider Nodes')
    await startRuntime()
    client = window.AgoraRTC.createClient({ mode: 'rtc', codec: 'vp8' })
    client.on('user-published', async (user, mediaType) => {
      diagnostic(`Agora remote media published · uid=${user.uid} type=${mediaType}`)
      await client.subscribe(user, mediaType)
      if (mediaType === 'audio') {
        user.audioTrack.play()
        $('#orb').classList.add('speaking')
        message('Voxa is speaking', 'Interrupt naturally — the VAD control plane will cancel stale output')
        setTimeout(() => $('#orb').classList.remove('speaking'), 900)
      }
    })
    client.on('user-joined', user => diagnostic(`Agora participant joined · uid=${user.uid}`))
    client.on('user-left', user => diagnostic(`Agora participant left · uid=${user.uid}`))
    client.on('user-unpublished', user => { $('#orb').classList.remove('speaking'); diagnostic(`Agora remote media unpublished · uid=${user.uid}`) })
    await client.join(connection.app_id, connection.channel, connection.web_token, Number(connection.web_uid))
    diagnostic(`Browser joined Agora · channel=${connection.channel} uid=${connection.web_uid}`)
    microphone = await window.AgoraRTC.createMicrophoneAudioTrack({
      encoderConfig: 'speech_standard',
      AEC: true,
      ANS: true,
      AGC: true,
    })
    diagnostic('Browser microphone track created · AEC/ANS/AGC enabled · local playback disabled')
    await client.publish([microphone])
    diagnostic('Browser microphone published to Agora')
    sessionStartedAt = Date.now()
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
    const nodeCalls = node => (node.prepare_total || 0) + (node.process_total || 0) + (node.signal_total || 0) + (node.finish_total || 0) + (node.abort_total || 0)
    $('#calls').textContent = (runtime.nodes || []).reduce((sum, node) => sum + nodeCalls(node), 0)
    $('#frames').textContent = (runtime.edges || []).reduce((sum, edge) => sum + (edge.enqueue_total || 0), 0)
    for (const stage of document.querySelectorAll('.pipeline div')) {
      const hint = stage.dataset.node
      stage.classList.toggle('active', live && (runtime.nodes || []).some(node => node.node_id.includes(hint) && nodeCalls(node) > 0))
    }
    const edge = id => (runtime.edges || []).find(value => value.edge_id === id)?.enqueue_total || 0
    const milestone = count => count === 0 ? 0 : 1 + Math.floor(count / 500)
    const pipelineState = `${milestone(edge('agora-input'))}/${milestone(edge('audio-to-qwen'))}/${milestone(edge('qwen-audio'))}/${milestone(edge('audio-to-room'))}`
    if (pipelineState !== lastPipelineState) {
      diagnostic(`Frames · Agora In=${edge('agora-input')} · Qwen In=${edge('audio-to-qwen')} · Qwen Out=${edge('qwen-audio')} · Agora Out=${edge('audio-to-room')}`)
      lastPipelineState = pipelineState
    }
    if (live && microphone && Date.now() - sessionStartedAt > 5000 && edge('agora-input') === 0) {
      message('Microphone published, but no native audio frames', 'Open .voxa/runtime.log and look for [VOXA][AGORA][audio.received]')
    }
    if (runtime.terminal?.kind && !['success', 'cancelled'].includes(runtime.terminal.kind)) {
      showError(new Error(`${runtime.terminal.code || 'VOXA-RUNTIME'} · ${runtime.terminal.message || runtime.terminal.kind}`))
    }
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
  sessionStartedAt = 0
  lastPipelineState = ''
  $('#launch').hidden = false
  $('#launch').disabled = false
  $('#leave').hidden = true
  $('#error').hidden = true
  message('Session ended', 'Start again whenever you are ready.')
  await pollRuntime()
}

$('#launch').addEventListener('click', join)
$('#leave').addEventListener('click', leave)
$('#copy-log').addEventListener('click', async () => {
  const text = [...$('#diagnostic-log').children].map(item => item.textContent).join('\n')
  try {
    await navigator.clipboard.writeText(text)
    diagnostic('Diagnostic log copied to clipboard')
  } catch (error) { showError(error) }
})
window.addEventListener('beforeunload', () => { microphone?.close(); client?.leave() })
pollRuntime()
