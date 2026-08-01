'use strict'

const NS = 'http://www.w3.org/2000/svg'
const ports = {
  'builtin.text_source': { inputs: [], outputs: ['text_out'], kind: 'source', label: 'Text Source' },
  'builtin.uppercase': { inputs: ['text_in'], outputs: ['text_out'], kind: 'transform', label: 'Uppercase' },
  'builtin.text_sink': { inputs: ['text_in'], outputs: [], kind: 'sink', label: 'Text Sink' },
}
const state = {
  token: '', graph: null, selected: null, positions: {}, diagnostics: [],
  history: [], future: [], dirty: false, zoom: 1, validating: null,
}

const $ = (selector) => document.querySelector(selector)
const $$ = (selector) => [...document.querySelectorAll(selector)]
const clone = (value) => JSON.parse(JSON.stringify(value))
const svg = (name, attributes = {}) => {
  const element = document.createElementNS(NS, name)
  for (const [key, value] of Object.entries(attributes)) element.setAttribute(key, value)
  return element
}

function bootToken() {
  const fragment = location.hash.slice(1)
  if (fragment) sessionStorage.setItem('voxa.studio.token', fragment)
  history.replaceState(null, '', location.pathname)
  return fragment || sessionStorage.getItem('voxa.studio.token') || ''
}

async function api(path, options = {}) {
  const headers = { Authorization: `Bearer ${state.token}`, ...(options.headers || {}) }
  const response = await fetch(path, { ...options, headers })
  const text = await response.text()
  let data = text
  try { data = JSON.parse(text) } catch (_) {}
  if (!response.ok) {
    const error = new Error(typeof data?.message === 'string' ? data.message : response.statusText)
    error.status = response.status
    error.data = data
    throw error
  }
  return data
}

async function loadStudio() {
  state.token = bootToken()
  if (!state.token) return fatal('The access token is missing from this Studio URL.')
  try {
    const [graph, metadata] = await Promise.all([api('/api/v1/graph'), api('/api/v1/studio')])
    state.graph = typeof graph === 'string' ? JSON.parse(graph) : graph
    $('#graph-path').textContent = metadata.graph_path
    $('#connection-status').textContent = metadata.writable ? 'Local runtime · writable' : 'Local runtime · read only'
    seedPositions()
    bindEvents()
    renderAll()
    await validateGraph(false)
  } catch (error) {
    fatal(error.status === 401 ? 'The Studio access token is invalid or expired.' : error.message)
  }
}

function bindEvents() {
  $$('[data-add-node]').forEach((button) => button.addEventListener('click', () => addNode(button.dataset.addNode)))
  $('#graph-id').addEventListener('change', (event) => mutate(() => { state.graph.graph_id = event.target.value.trim() }))
  $('#node-id').addEventListener('change', updateSelectedNode)
  $('#node-type').addEventListener('change', updateSelectedNode)
  $('#node-config').addEventListener('change', updateSelectedNode)
  $('#node-config').addEventListener('blur', updateSelectedNode)
  $('#delete-node').addEventListener('click', deleteSelectedNode)
  $('#add-edge').addEventListener('click', openEdgeDialog)
  $('#edge-form').addEventListener('submit', submitEdge)
  $('#edge-from-node').addEventListener('change', refreshEdgePorts)
  $('#edge-to-node').addEventListener('change', refreshEdgePorts)
  $('#validate').addEventListener('click', () => validateGraph(true))
  $('#save').addEventListener('click', saveGraph)
  $('#undo').addEventListener('click', undo)
  $('#redo').addEventListener('click', redo)
  $('#raw-toggle').addEventListener('click', openRaw)
  $('#raw-close').addEventListener('click', closeRaw)
  $('#format-json').addEventListener('click', formatRaw)
  $('#apply-json').addEventListener('click', applyRaw)
  $('#zoom-in').addEventListener('click', () => setZoom(state.zoom + .1))
  $('#zoom-out').addEventListener('click', () => setZoom(state.zoom - .1))
  $('#fit-view').addEventListener('click', fitView)
  $('#graph-canvas').addEventListener('click', (event) => { if (event.target.id === 'graph-canvas') selectNode(null) })
  window.addEventListener('keydown', keyboardShortcut)
}

function keyboardShortcut(event) {
  const command = event.metaKey || event.ctrlKey
  if (command && event.key.toLowerCase() === 's') { event.preventDefault(); saveGraph() }
  if (command && event.key.toLowerCase() === 'z') { event.preventDefault(); event.shiftKey ? redo() : undo() }
  if ((event.key === 'Delete' || event.key === 'Backspace') && state.selected && !['INPUT', 'TEXTAREA'].includes(document.activeElement.tagName)) deleteSelectedNode()
}

function seedPositions() {
  const columns = { source: 0, transform: 0, sink: 0 }
  const x = { source: 110, transform: 475, sink: 840 }
  state.graph.nodes.forEach((node, index) => {
    const info = ports[node.node_type] || { kind: 'transform' }
    const row = columns[info.kind]++
    state.positions[node.id] = { x: x[info.kind] ?? 180 + index * 260, y: 130 + row * 175 }
  })
}

function snapshot() { return JSON.stringify(state.graph) }
function mutate(operation) {
  state.history.push(snapshot())
  if (state.history.length > 80) state.history.shift()
  state.future = []
  operation()
  setDirty(true)
  renderAll()
  scheduleValidation()
}
function undo() {
  if (!state.history.length) return
  state.future.push(snapshot())
  state.graph = JSON.parse(state.history.pop())
  reconcilePositions(); setDirty(true); renderAll(); scheduleValidation()
}
function redo() {
  if (!state.future.length) return
  state.history.push(snapshot())
  state.graph = JSON.parse(state.future.pop())
  reconcilePositions(); setDirty(true); renderAll(); scheduleValidation()
}
function reconcilePositions() {
  const retained = {}
  state.graph.nodes.forEach((node, index) => { retained[node.id] = state.positions[node.id] || { x: 120 + index * 260, y: 180 } })
  state.positions = retained
  if (!state.graph.nodes.some((node) => node.id === state.selected)) state.selected = null
}

function addNode(type) {
  const info = ports[type]
  const base = info.kind === 'source' ? 'source' : info.kind === 'sink' ? 'sink' : 'transform'
  let number = 1
  while (state.graph.nodes.some((node) => node.id === `${base}-${number}`)) number++
  const id = `${base}-${number}`
  mutate(() => {
    state.graph.nodes.push({ id, node_type: type, language: 'rust', node_config: type === 'builtin.text_source' ? { text: 'hello' } : {} })
    const sameKind = state.graph.nodes.filter((node) => ports[node.node_type]?.kind === info.kind).length - 1
    const x = info.kind === 'source' ? 110 : info.kind === 'sink' ? 840 : 475
    state.positions[id] = { x, y: 130 + sameKind * 175 }
    state.selected = id
  })
}

function deleteSelectedNode() {
  if (!state.selected) return
  const id = state.selected
  mutate(() => {
    state.graph.nodes = state.graph.nodes.filter((node) => node.id !== id)
    state.graph.edges = state.graph.edges.filter((edge) => edge.from.node_id !== id && edge.to.node_id !== id)
    delete state.positions[id]
    state.selected = null
  })
}

function updateSelectedNode() {
  const node = selectedNode()
  if (!node) return
  let config
  try { config = JSON.parse($('#node-config').value); $('#config-error').textContent = '' }
  catch (error) { $('#config-error').textContent = error.message; return }
  if (!config || Array.isArray(config) || typeof config !== 'object') { $('#config-error').textContent = 'Configuration must be a JSON object'; return }
  const nextId = $('#node-id').value.trim()
  if (!nextId) { $('#config-error').textContent = 'Node ID is required'; return }
  if (state.graph.nodes.some((candidate) => candidate !== node && candidate.id === nextId)) { $('#config-error').textContent = 'Node ID must be unique'; return }
  if (node.id === nextId && node.node_type === $('#node-type').value && JSON.stringify(node.node_config) === JSON.stringify(config)) return
  const previousId = node.id
  mutate(() => {
    node.id = nextId
    node.node_type = $('#node-type').value
    node.node_config = config
    if (previousId !== nextId) {
      state.graph.edges.forEach((edge) => {
        if (edge.from.node_id === previousId) edge.from.node_id = nextId
        if (edge.to.node_id === previousId) edge.to.node_id = nextId
      })
      state.positions[nextId] = state.positions[previousId]
      delete state.positions[previousId]
      state.selected = nextId
    }
  })
}

function openEdgeDialog() {
  const sources = state.graph.nodes.filter((node) => ports[node.node_type]?.outputs.length)
  const targets = state.graph.nodes.filter((node) => ports[node.node_type]?.inputs.length)
  fillSelect($('#edge-from-node'), sources)
  fillSelect($('#edge-to-node'), targets)
  refreshEdgePorts()
  $('#edge-dialog').showModal()
}
function fillSelect(select, nodes) {
  select.replaceChildren(...nodes.map((node) => { const option = document.createElement('option'); option.value = node.id; option.textContent = node.id; return option }))
}
function refreshEdgePorts() {
  const source = state.graph.nodes.find((node) => node.id === $('#edge-from-node').value)
  const target = state.graph.nodes.find((node) => node.id === $('#edge-to-node').value)
  fillStringSelect($('#edge-from-port'), ports[source?.node_type]?.outputs || [])
  fillStringSelect($('#edge-to-port'), ports[target?.node_type]?.inputs || [])
}
function fillStringSelect(select, values) {
  select.replaceChildren(...values.map((value) => { const option = document.createElement('option'); option.value = value; option.textContent = value; return option }))
}
function submitEdge(event) {
  if (event.submitter?.value === 'cancel') return
  event.preventDefault()
  const from = $('#edge-from-node').value, to = $('#edge-to-node').value
  if (!from || !to) return toast('Add compatible source and target nodes first', true)
  let base = `${from}-${to}`, id = base, number = 2
  while (state.graph.edges.some((edge) => edge.id === id)) id = `${base}-${number++}`
  mutate(() => state.graph.edges.push({
    id, from: { node_id: from, port: $('#edge-from-port').value }, to: { node_id: to, port: $('#edge-to-port').value },
    frame_type: 'text', queue_policy: { capacity: Number($('#edge-capacity').value), overflow: $('#edge-overflow').value },
  }))
  $('#edge-dialog').close()
}
function deleteEdge(id) { mutate(() => { state.graph.edges = state.graph.edges.filter((edge) => edge.id !== id) }) }

function renderAll() {
  $('#graph-id').value = state.graph.graph_id
  $('#raw-json').value = JSON.stringify(state.graph, null, 2)
  $('#undo').disabled = !state.history.length
  $('#redo').disabled = !state.future.length
  renderCanvas(); renderEdgesList(); renderInspector()
}

function renderCanvas() {
  const edgeLayer = $('#edge-layer'), nodeLayer = $('#node-layer')
  renderEdgeLayer(edgeLayer)
  nodeLayer.replaceChildren()
  for (const node of state.graph.nodes) nodeLayer.append(renderNode(node))
  const width = 1200 / state.zoom, height = 760 / state.zoom
  $('#graph-canvas').setAttribute('viewBox', `${(1200 - width) / 2} ${(760 - height) / 2} ${width} ${height}`)
}

function renderEdgeLayer(edgeLayer = $('#edge-layer')) {
  edgeLayer.replaceChildren()
  for (const edge of state.graph.edges) {
    const from = state.positions[edge.from.node_id], to = state.positions[edge.to.node_id]
    if (!from || !to) continue
    const x1 = from.x + 220, y1 = from.y + 68, x2 = to.x, y2 = to.y + 68
    const bend = Math.max(80, Math.abs(x2 - x1) * .48)
    edgeLayer.append(svg('path', { d: `M${x1},${y1} C${x1 + bend},${y1} ${x2 - bend},${y2} ${x2},${y2}`, class: 'graph-edge', 'data-edge': edge.id }))
  }
}

function renderNode(node) {
  const info = ports[node.node_type] || { kind: 'transform', label: node.node_type, inputs: [], outputs: [] }
  const position = state.positions[node.id] || { x: 100, y: 100 }
  const group = svg('g', { class: `node-group${node.id === state.selected ? ' selected' : ''}`, transform: `translate(${position.x} ${position.y})`, 'data-node': node.id, tabindex: '0' })
  group.append(svg('rect', { class: 'node-card', width: 220, height: 108, rx: 11 }))
  group.append(svg('rect', { class: `node-accent ${info.kind}`, width: 4, height: 78, x: 0, y: 15, rx: 2 }))
  addText(group, 19, 27, info.kind.toUpperCase(), 'node-kind-label')
  addText(group, 19, 51, node.id, 'node-title')
  addText(group, 19, 72, node.node_type, 'node-type-label')
  addText(group, 19, 94, 'rust · text', 'node-type-label')
  info.inputs.forEach((name, index) => { group.append(svg('circle', { cx: 0, cy: 68 + index * 18, r: 5, class: 'port-dot input' })); addText(group, 9, 72 + index * 18, name, 'port-label') })
  info.outputs.forEach((name, index) => { group.append(svg('circle', { cx: 220, cy: 68 + index * 18, r: 5, class: 'port-dot output' })); const text = addText(group, 211, 72 + index * 18, name, 'port-label'); text.setAttribute('text-anchor', 'end') })
  group.addEventListener('click', (event) => { event.stopPropagation(); selectNode(node.id) })
  group.addEventListener('pointerdown', (event) => beginDrag(event, node.id))
  return group
}
function addText(parent, x, y, value, className) { const text = svg('text', { x, y, class: className }); text.textContent = value; parent.append(text); return text }

function beginDrag(event, id) {
  if (event.button !== 0) return
  const target = event.currentTarget
  target.setPointerCapture(event.pointerId)
  const start = { clientX: event.clientX, clientY: event.clientY, nodeX: state.positions[id].x, nodeY: state.positions[id].y }
  const move = (next) => {
    state.positions[id] = { x: start.nodeX + (next.clientX - start.clientX) / state.zoom, y: start.nodeY + (next.clientY - start.clientY) / state.zoom }
    target.setAttribute('transform', `translate(${state.positions[id].x} ${state.positions[id].y})`)
    renderEdgeLayer()
  }
  const stop = () => {
    target.removeEventListener('pointermove', move)
    target.removeEventListener('pointerup', stop)
    target.removeEventListener('pointercancel', stop)
    if (target.hasPointerCapture(event.pointerId)) target.releasePointerCapture(event.pointerId)
  }
  target.addEventListener('pointermove', move)
  target.addEventListener('pointerup', stop)
  target.addEventListener('pointercancel', stop)
}

function renderEdgesList() {
  const list = $('#edge-list')
  if (!state.graph.edges.length) { const empty = document.createElement('div'); empty.className = 'edge-row'; empty.textContent = 'No edges yet'; list.replaceChildren(empty); return }
  list.replaceChildren(...state.graph.edges.map((edge) => {
    const row = document.createElement('div'); row.className = 'edge-row'
    const route = document.createElement('div'); route.className = 'edge-route'
    const from = document.createElement('b'); from.textContent = edge.from.node_id
    const to = document.createElement('b'); to.textContent = edge.to.node_id
    route.append(from, document.createTextNode(' → '), to)
    const remove = document.createElement('button'); remove.className = 'edge-delete'; remove.textContent = '×'; remove.title = `Delete ${edge.id}`; remove.addEventListener('click', () => deleteEdge(edge.id))
    row.append(route, remove); return row
  }))
}

function selectedNode() { return state.graph.nodes.find((node) => node.id === state.selected) }
function selectNode(id) { state.selected = id; renderCanvas(); renderInspector() }
function renderInspector() {
  const node = selectedNode(), empty = $('#empty-inspector'), form = $('#node-inspector')
  $('#delete-node').disabled = !node
  empty.classList.toggle('hidden', Boolean(node)); form.classList.toggle('hidden', !node)
  if (!node) return
  $('#node-id').value = node.id; $('#node-type').value = node.node_type; $('#node-language').value = node.language; $('#node-config').value = JSON.stringify(node.node_config, null, 2); $('#config-error').textContent = ''
}

function scheduleValidation() {
  clearTimeout(state.validating)
  state.validating = setTimeout(() => validateGraph(false), 300)
}
async function validateGraph(notify) {
  clearTimeout(state.validating)
  try {
    state.diagnostics = await api('/api/v1/graph/validate', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(state.graph) })
    if (notify) toast('Graph is valid')
  } catch (error) {
    state.diagnostics = Array.isArray(error.data) ? error.data : [{ code: 'VOXA-STUDIO', pointer: '', message: error.message }]
    if (notify) toast('Graph has validation errors', true)
  }
  renderDiagnostics()
  return state.diagnostics.length === 0
}
function renderDiagnostics() {
  $('#diagnostic-count').textContent = state.diagnostics.length
  const container = $('#diagnostics')
  if (!state.diagnostics.length) { const valid = document.createElement('div'); valid.className = 'valid-message'; valid.textContent = '✓ Graph v1 is valid'; container.replaceChildren(valid); return }
  container.replaceChildren(...state.diagnostics.map((diagnostic) => {
    const item = document.createElement('div'); item.className = 'diagnostic'
    const code = document.createElement('b'); code.textContent = diagnostic.code
    const message = document.createElement('span'); message.textContent = `${diagnostic.pointer || '/'} · ${diagnostic.message}`
    item.append(code, message)
    item.addEventListener('click', () => { const match = diagnostic.pointer?.match(/^\/nodes\/(\d+)/); if (match) selectNode(state.graph.nodes[Number(match[1])]?.id || null) })
    return item
  }))
}

async function saveGraph() {
  if (!await validateGraph(false)) return toast('Fix validation errors before saving', true)
  try {
    await api('/api/v1/graph', { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(state.graph) })
    setDirty(false); toast('Graph saved to disk')
  } catch (error) { toast(error.message, true) }
}
function setDirty(dirty) { state.dirty = dirty; $('#dirty-dot').classList.toggle('dirty', dirty); $('#dirty-dot').title = dirty ? 'Unsaved changes' : 'Saved' }

function openRaw() { $('#raw-json').value = JSON.stringify(state.graph, null, 2); $('#raw-drawer').classList.add('open'); $('#raw-drawer').setAttribute('aria-hidden', 'false') }
function closeRaw() { $('#raw-drawer').classList.remove('open'); $('#raw-drawer').setAttribute('aria-hidden', 'true') }
function formatRaw() {
  try { $('#raw-json').value = JSON.stringify(JSON.parse($('#raw-json').value), null, 2); $('#raw-error').textContent = '' }
  catch (error) { $('#raw-error').textContent = error.message }
}
function applyRaw() {
  try {
    const graph = JSON.parse($('#raw-json').value)
    mutate(() => { state.graph = graph; state.selected = null; reconcilePositions() })
    $('#raw-error').textContent = ''; closeRaw()
  } catch (error) { $('#raw-error').textContent = error.message }
}

function setZoom(value) { state.zoom = Math.min(1.4, Math.max(.6, Number(value.toFixed(1)))); $('#zoom-label').textContent = `${Math.round(state.zoom * 100)}%`; renderCanvas() }
function fitView() { setZoom(state.graph.nodes.length > 5 ? .8 : 1) }
function toast(message, error = false) {
  const element = $('#toast'); element.textContent = message; element.classList.toggle('error', error); element.classList.add('show')
  clearTimeout(toast.timer); toast.timer = setTimeout(() => element.classList.remove('show'), 2400)
}
function fatal(message) { $('#fatal-message').textContent = message; $('#fatal').classList.remove('hidden') }

loadStudio()
