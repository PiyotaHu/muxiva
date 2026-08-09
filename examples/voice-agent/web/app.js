'use strict'

const backendKey = 'muxiva.voice.backend'
const clientTokenKey = 'muxiva.voice.client-token'
const queryBackend = new URLSearchParams(location.search).get('backend')
const initialBackend = queryBackend || localStorage.getItem(backendKey) || 'http://127.0.0.1:8080'
const $ = (selector) => document.querySelector(selector)
$('#backend-url').value = initialBackend
$('#client-token').value = sessionStorage.getItem(clientTokenKey) || ''
let client = null
let microphone = null
let remoteAudioTrack = null
let remoteAudioObserved = false
let meterTimer = null
let sessionStartedAt = 0
let lastErrorMessage = ''
let currentUserMessage = null
let currentAgentMessage = null
let lastCompletedTranscriptSequence = -1
let transcriptCommittedForCurrentUtterance = false
let botUid = null
let clientMessageCount = 0
let microphoneEnergyObserved = false
let microphoneWarningShown = false
const fragments = new Map()
const maximumFragmentMessages = 64
const maximumFragmentsPerMessage = 64

function showBargeState(mode, label) {
  const status = $('#barge-status')
  status.className = `barge-status ${mode || ''}`.trim()
  status.querySelector('span').textContent = label
}

for (let index = 0; index < 32; index += 1) {
  const level = document.createElement('i')
  $('#levels').append(level)
}

async function api(path, options = {}) {
  const backend = $('#backend-url').value.trim().replace(/\/$/, '')
  if (!/^https?:\/\//.test(backend)) throw new Error('Backend URL must start with http:// or https://')
  const token = $('#client-token').value.trim()
  const authorization = token ? { Authorization: `Bearer ${token}` } : {}
  const response = await fetch(new URL(path, `${backend}/`), {
    ...options,
    headers: { ...authorization, ...(options.headers || {}) },
  })
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

function resetConversation() {
  $('#message-list').replaceChildren()
  const empty = document.createElement('div')
  empty.id = 'conversation-empty'
  empty.className = 'conversation-empty'
  const title = document.createElement('b')
  title.textContent = 'Your conversation will appear here'
  const detail = document.createElement('span')
  detail.textContent = 'You on the right · Muxiva on the left'
  empty.append(title, detail)
  $('#message-list').append(empty)
  currentUserMessage = null
  currentAgentMessage = null
  lastCompletedTranscriptSequence = -1
  transcriptCommittedForCurrentUtterance = false
}

function createChatMessage(role) {
  $('#conversation-empty')?.remove()
  const article = document.createElement('article')
  article.className = `chat-message ${role} streaming`
  const body = document.createElement('div')
  body.className = 'message-body'
  const label = document.createElement('small')
  label.textContent = role === 'user' ? 'YOU · ASR' : 'MUXIVA · STREAMING RESPONSE'
  const copy = document.createElement('p')
  body.append(label, copy)
  article.append(body)
  $('#message-list').append(article)
  while ($('#message-list').children.length > 50) $('#message-list').firstElementChild.remove()
  $('#message-list').scrollTop = $('#message-list').scrollHeight
  return { article, copy }
}

function beginUserMessage() {
  currentAgentMessage?.article.classList.remove('streaming')
  currentAgentMessage = null
  currentUserMessage?.article.classList.remove('streaming')
  currentUserMessage = createChatMessage('user')
  currentUserMessage.copy.textContent = 'Listening…'
}

function previewUserMessage(text) {
  if (!currentUserMessage) currentUserMessage = createChatMessage('user')
  currentUserMessage.copy.textContent = text
  $('#message-list').scrollTop = $('#message-list').scrollHeight
}

function completeUserMessage(text) {
  previewUserMessage(text)
  currentUserMessage.article.classList.remove('streaming')
  currentUserMessage = null
}

function appendAgentMessage(text) {
  if (!currentAgentMessage) currentAgentMessage = createChatMessage('agent')
  currentAgentMessage.copy.textContent += text
  $('#message-list').scrollTop = $('#message-list').scrollHeight
}

function completeAgentMessage(text = '') {
  if (text && !currentAgentMessage) {
    currentAgentMessage = createChatMessage('agent')
    currentAgentMessage.copy.textContent = text
  }
  currentAgentMessage?.article.classList.remove('streaming')
  currentAgentMessage = null
}

function diagnostic(text, error = false, warning = false) {
  const item = document.createElement('li')
  if (error) item.className = 'error-line'
  else if (warning) item.className = 'warning-line'
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
  $('#error').textContent = `${error.message}\n\nVerify the Backend URL, MUXIVA_CLIENT_API_TOKEN (remote deployments), Agora browser identity, and the headless Runtime log.`
  if (error.message !== lastErrorMessage) {
    diagnostic(`ERROR · ${error.message}`, true)
    lastErrorMessage = error.message
  }
}

async function join() {
    $('#launch').disabled = true
    $('#error').hidden = true
    lastErrorMessage = ''
    $('#diagnostic-log').replaceChildren()
    resetConversation()
    showBargeState('', 'VOICE CONTROL READY')
  try {
    saveBackendConfiguration()
    if (!window.AgoraRTC) throw new Error('Agora Web SDK could not be loaded.')
    const connection = (await api('/api/v1/client/session')).agora || {}
    for (const field of ['app_id', 'channel', 'bot_uid', 'web_uid', 'web_token']) {
      if (!connection[field]) throw new Error(`Agora browser field ${field} is not configured.`)
    }
    botUid = String(connection.bot_uid)
    message('Joining the voice session…', 'The Muxiva Runtime must already be running')
    diagnostic('Joining the production RTC media and message transports')
    client = window.AgoraRTC.createClient({ mode: 'rtc', codec: 'vp8' })
    client.on('user-published', async (user, mediaType) => {
      if (String(user.uid) !== botUid) {
        diagnostic(`Ignoring media from unexpected participant · uid=${user.uid}`)
        return
      }
      diagnostic(`Agora remote media published · uid=${user.uid} type=${mediaType}`)
      await client.subscribe(user, mediaType)
      if (mediaType === 'audio') {
        remoteAudioTrack = user.audioTrack
        remoteAudioObserved = false
        remoteAudioTrack.setVolume?.(100)
        remoteAudioTrack.play()
        diagnostic(`Assistant audio subscribed and playing · uid=${user.uid}`)
        $('#orb').classList.add('speaking')
        message('Muxiva is speaking', 'Interrupt naturally — the VAD control plane will cancel stale output')
        setTimeout(() => $('#orb').classList.remove('speaking'), 900)
      }
    })
    client.on('user-joined', user => diagnostic(`Agora participant joined · uid=${user.uid}`))
    client.on('user-left', user => diagnostic(`Agora participant left · uid=${user.uid}`))
    client.on('user-unpublished', user => { remoteAudioTrack = null; $('#orb').classList.remove('speaking'); diagnostic(`Agora remote media unpublished · uid=${user.uid}`) })
    client.on('stream-message', (uid, payload) => {
      if (String(uid) !== botUid) return
      handleTransportMessage(payload)
    })
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
    clientMessageCount = 0
    microphoneEnergyObserved = false
    microphoneWarningShown = false
    $('#orb').classList.add('live')
    $('#orb span').textContent = 'LIVE'
    $('#launch').hidden = true
    $('#leave').hidden = false
    message('Listening — say something', 'This session stays open. You can speak over the assistant at any time.')
    $('#runtime-pill').classList.add('live')
    $('#runtime-pill b').textContent = 'RTC session live'
    $('#graph-name').textContent = connection.channel
    startMeter()
  } catch (error) {
    $('#launch').disabled = false
    showError(error)
  }
}

function saveBackendConfiguration() {
  const backend = $('#backend-url').value.trim().replace(/\/$/, '')
  localStorage.setItem(backendKey, backend)
  const token = $('#client-token').value.trim()
  if (token) sessionStorage.setItem(clientTokenKey, token)
  else sessionStorage.removeItem(clientTokenKey)
}

async function testBackend() {
  const state = $('#backend-state')
  state.className = ''
  state.textContent = 'Testing…'
  $('#test-backend').disabled = true
  try {
    saveBackendConfiguration()
    const health = await api('/healthz')
    const session = await api('/api/v1/client/session')
    if (!session.agora) throw new Error('Runtime responded, but Agora browser configuration is missing')
    state.className = 'ready'
    state.textContent = `Connected · Graph ${health.graph_id} · Channel ${session.agora.channel || 'not configured'}`
  } catch (error) {
    state.className = 'failed'
    state.textContent = `Connection failed · ${error.message}`
  } finally {
    $('#test-backend').disabled = false
  }
}

function startMeter() {
  clearInterval(meterTimer)
  meterTimer = setInterval(() => {
    const volume = microphone?.getVolumeLevel?.() || 0
    $('#mic-level').textContent = `${Math.round(volume * 100)}%`
    if (volume >= 0.015 && !microphoneEnergyObserved) {
      microphoneEnergyObserved = true
      diagnostic(`Microphone speech energy detected · level=${Math.round(volume * 100)}%`)
    }
    if (!microphoneEnergyObserved && !microphoneWarningShown && sessionStartedAt && Date.now() - sessionStartedAt >= 5000) {
      microphoneWarningShown = true
      diagnostic('No microphone speech energy detected yet · check the selected input device and browser microphone permission', false, true)
    }
    const bars = [...document.querySelectorAll('#levels i')]
    bars.forEach((bar, index) => {
      const curve = Math.sin((index / bars.length) * Math.PI)
      const jitter = .35 + Math.random() * .65
      bar.style.height = `${5 + Math.max(volume * 90 * curve * jitter, 1)}px`
    })
  }, 80)
}

function decodeBase64(value) {
  const binary = atob(value)
  return Uint8Array.from(binary, character => character.charCodeAt(0))
}

function handleTransportMessage(payload) {
  try {
    const bytes = payload instanceof Uint8Array ? payload : new Uint8Array(payload)
    const message = JSON.parse(new TextDecoder().decode(bytes))
    if (message.version === 'muxiva.transport-fragment/v1') {
      if (typeof message.message_id !== 'string' || message.message_id.length > 128 ||
          !Number.isInteger(message.fragment_count) || message.fragment_count < 1 ||
          message.fragment_count > maximumFragmentsPerMessage ||
          !Number.isInteger(message.fragment_index) || message.fragment_index < 0 ||
          message.fragment_index >= message.fragment_count || typeof message.data !== 'string') {
        throw new Error('fragment envelope is outside protocol limits')
      }
      if (!fragments.has(message.message_id) && fragments.size >= maximumFragmentMessages) {
        fragments.delete(fragments.keys().next().value)
      }
      const state = fragments.get(message.message_id) || { count: message.fragment_count, chunks: [] }
      if (state.count !== message.fragment_count) throw new Error('fragment count changed')
      state.chunks[message.fragment_index] = decodeBase64(message.data)
      fragments.set(message.message_id, state)
      if (state.chunks.filter(Boolean).length !== state.count) return
      const size = state.chunks.reduce((sum, chunk) => sum + chunk.length, 0)
      const joined = new Uint8Array(size)
      let offset = 0
      for (const chunk of state.chunks) { joined.set(chunk, offset); offset += chunk.length }
      fragments.delete(message.message_id)
      handleClientEvent(JSON.parse(new TextDecoder().decode(joined)))
      return
    }
    handleClientEvent(message)
  } catch (error) {
    diagnostic(`Invalid Muxiva RTC message · ${error.message}`, true)
  }
}

function handleClientEvent(event) {
    if (event.version !== 'muxiva.client-event/v1' || typeof event.type !== 'string') return
    clientMessageCount += 1
    $('#calls').textContent = clientMessageCount
    $('#frames').textContent = event.sequence || clientMessageCount
    const text = typeof event.payload?.text === 'string' ? event.payload.text : ''
    const numericSequence = Number(event.sequence)
    const sequence = Number.isSafeInteger(numericSequence) && numericSequence >= 0
      ? numericSequence
      : null
    if (event.type === 'muxiva.voice.speech.started') {
      transcriptCommittedForCurrentUtterance = false
      const interruptedPlayback = Boolean(currentAgentMessage) || $('#orb').classList.contains('speaking')
      beginUserMessage()
      if (interruptedPlayback) {
        showBargeState('interrupting', 'BARGE-IN · INTERRUPTING AGENT')
        message('Listening — old response cancelled', 'Speech-start Signal cancelled LLM, TTS, and playback')
        diagnostic('Barge-in · cascade generation, synthesis, and Agora output are cancelling')
      } else {
        showBargeState('listening', 'YOU ARE SPEAKING')
        message('Listening — speak naturally', 'Speech-start Signal entered the Muxiva control plane')
      }
    } else if (event.type === 'muxiva.voice.barge_in') {
      showBargeState('interrupting', 'BARGE-IN · INTERRUPTING AGENT')
      diagnostic('Barge-in · Qwen generation cancelled; Agora output queue clearing')
    } else if (event.type === 'muxiva.voice.speech.stopped') {
      showBargeState('', 'UTTERANCE CAPTURED')
    } else if (event.type === 'muxiva.voice.transcript.preview') {
      if (transcriptCommittedForCurrentUtterance) return
      if (sequence !== null && sequence < lastCompletedTranscriptSequence) return
      previewUserMessage(text)
    } else if (event.type === 'muxiva.voice.transcript.delta') {
      if (transcriptCommittedForCurrentUtterance) return
      if (sequence !== null && sequence < lastCompletedTranscriptSequence) return
      previewUserMessage(`${currentUserMessage?.copy.textContent || ''}${text}`)
    } else if (event.type === 'muxiva.voice.transcript.completed') {
      if (transcriptCommittedForCurrentUtterance) return
      if (sequence !== null && sequence < lastCompletedTranscriptSequence) return
      transcriptCommittedForCurrentUtterance = true
      if (sequence !== null) lastCompletedTranscriptSequence = Math.max(lastCompletedTranscriptSequence, sequence)
      completeUserMessage(text)
      showBargeState('', 'VOICE CONTROL READY')
      message('Thinking…', 'Transcript committed to the typed Graph')
    } else if (event.type === 'muxiva.voice.response.delta') {
      appendAgentMessage(text)
      message('Muxiva is responding', 'Text and audio are streaming through separate typed branches')
    } else if (event.type === 'muxiva.voice.response.completed') {
      completeAgentMessage(text)
    } else if (event.type === 'muxiva.voice.transcript.failed') {
      diagnostic(`ASR failed · ${event.payload?.message || 'unknown error'}`, true)
    }
}

async function leave() {
  clearInterval(meterTimer)
  if (microphone) { microphone.stop(); microphone.close(); microphone = null }
  if (client) { await client.leave(); client = null }
  remoteAudioTrack = null
  remoteAudioObserved = false
  $('#orb').className = 'orb'
  $('#orb span').textContent = 'READY'
  sessionStartedAt = 0
  microphoneEnergyObserved = false
  microphoneWarningShown = false
  $('#mic-level').textContent = '0%'
  fragments.clear()
  showBargeState('', 'VOICE CONTROL READY')
  $('#launch').hidden = false
  $('#launch').disabled = false
  $('#leave').hidden = true
  $('#error').hidden = true
  message('Session ended', 'Start again whenever you are ready.')
  $('#runtime-pill').classList.remove('live')
  $('#runtime-pill b').textContent = 'RTC session idle'
}

$('#launch').addEventListener('click', join)
$('#leave').addEventListener('click', leave)
$('#test-backend').addEventListener('click', testBackend)
$('#copy-log').addEventListener('click', async () => {
  const text = [...$('#diagnostic-log').children].map(item => item.textContent).join('\n')
  try {
    await navigator.clipboard.writeText(text)
    diagnostic('Diagnostic log copied to clipboard')
  } catch (error) { showError(error) }
})
window.addEventListener('beforeunload', () => { microphone?.close(); client?.leave() })
