'use strict'

const NS = 'http://www.w3.org/2000/svg'
const catalog = new Map()
let nodePackages = []
const state = {
  token: '', graph: null, selected: null, positions: {}, diagnostics: [],
  history: [], future: [], dirty: false, zoom: 1, validating: null,
  runtime: { status: 'idle', nodes: [], edges: [] }, runtimeTimer: null,
  connection: null, editingEdge: null,
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
    const [graph, metadata, registrations, packages, runtime] = await Promise.all([
      api('/api/v1/graph'), api('/api/v1/studio'), api('/api/v1/registry/nodes'), api('/api/v1/node-library'), api('/api/v1/runtime'),
    ])
    state.graph = typeof graph === 'string' ? JSON.parse(graph) : graph
    state.runtime = runtime
    nodePackages = packages
    installCatalog([...registrations, ...packages.map(packageCatalogEntry)])
    renderPalette()
    $('#graph-path').textContent = metadata.graph_path
    $('#connection-status').textContent = metadata.writable ? 'Local runtime · writable' : 'Local runtime · read only'
    seedPositions()
    bindEvents()
    renderAll()
    await validateGraph(false)
    scheduleRuntimePoll()
  } catch (error) {
    fatal(error.status === 401 ? 'The Studio access token is invalid or expired.' : error.message)
  }
}

function factoryKey(value) { return JSON.stringify([value.node_type, value.language, value.factory_version]) }
function packageCatalogEntry(value) { return { ...value, runtime_available: value.runtime_available } }
function nodeInfo(node) {
  return catalog.get(factoryKey(node)) || {
    kind: 'transform', label: node.node_type, language: node.language || 'unknown',
    inputs: [], outputs: [], inputPorts: [], outputPorts: [], frameTypes: [], config_schema: {},
  }
}
function installCatalog(registrations) {
  if (!Array.isArray(registrations) || !registrations.length) throw new Error('The runtime Node Registry is empty.')
  for (const entry of registrations) {
    const info = { ...entry, inputs: [], outputs: [], inputPorts: [], outputPorts: [], frameTypes: [...new Set((entry.ports || []).map((port) => port.frame_type))] }
    for (const port of entry.ports || []) {
      const direction = port.direction === 'input' ? 'input' : 'output'
      info[`${direction}s`].push(port.name)
      info[`${direction}Ports`].push(port)
    }
    info.label = entry.node_type.split('.').pop().split('_').map((part) => part[0].toUpperCase() + part.slice(1)).join(' ')
    catalog.set(factoryKey(entry), info)
  }
}
function renderPalette() {
  const buttons = [...catalog.entries()].map(([key, entry]) => {
    const button = document.createElement('button'); button.className = `palette-item ${entry.kind}`; button.dataset.addNode = key; button.draggable = true
    const icon = document.createElement('span'); icon.className = 'node-icon'; icon.textContent = entry.kind[0].toUpperCase()
    const copy = document.createElement('span'), label = document.createElement('b'), detail = document.createElement('small')
    label.textContent = entry.display_name || entry.label; detail.textContent = `${entry.language} · v${entry.factory_version}${entry.package_id ? ' · project' : ''}`; copy.append(label, detail)
    const add = document.createElement('span'); add.textContent = '＋'; button.append(icon, copy, add); return button
  })
  $('#node-palette').replaceChildren(...buttons)
  $('#node-type').replaceChildren(...[...catalog.entries()].map(([key, entry]) => {
    const option = document.createElement('option'); option.value = key; option.textContent = `${entry.node_type} · ${entry.language} · v${entry.factory_version}`; return option
  }))
}
function defaultConfig(entry) {
  const result = {}, properties = entry.config_schema?.properties || {}
  for (const name of entry.config_schema?.required || []) {
    const property = properties[name] || {}
    if (Object.hasOwn(property, 'default')) result[name] = clone(property.default)
    else if (property.type === 'string') result[name] = ''
  }
  return result
}

function bindEvents() {
  bindPaletteEvents()
  $('#graph-id').addEventListener('change', (event) => mutate(() => { state.graph.graph_id = event.target.value.trim() }))
  $('#node-id').addEventListener('change', updateSelectedNode)
  $('#node-type').addEventListener('change', updateSelectedNode)
  $('#node-config').addEventListener('change', updateSelectedNode)
  $('#node-config').addEventListener('blur', updateSelectedNode)
  $('#delete-node').addEventListener('click', deleteSelectedNode)
  $('#add-edge').addEventListener('click', () => openEdgeDialog())
  $('#open-node-lab').addEventListener('click', openNodeLab)
  $('#node-lab-close').addEventListener('click', closeNodeLab)
  $('#node-lab-cancel').addEventListener('click', closeNodeLab)
  $('#node-lab-language').addEventListener('change', applyNodeTemplate)
  $('#node-lab-form').addEventListener('submit', saveNodePackage)
  $('#edge-form').addEventListener('submit', submitEdge)
  $('#edge-from-node').addEventListener('change', refreshEdgePorts)
  $('#edge-from-port').addEventListener('change', refreshCompatibleInputPorts)
  $('#edge-to-node').addEventListener('change', refreshCompatibleInputPorts)
  $('#validate').addEventListener('click', () => validateGraph(true))
  $('#run').addEventListener('click', startRuntime)
  $('#stop').addEventListener('click', stopRuntime)
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
  $('#graph-canvas').addEventListener('dragover', (event) => { event.preventDefault(); event.dataTransfer.dropEffect = 'copy' })
  $('#graph-canvas').addEventListener('drop', dropPaletteNode)
  window.addEventListener('keydown', keyboardShortcut)
}

function bindPaletteEvents() {
  $$('[data-add-node]').forEach((button) => button.addEventListener('click', () => addNode(button.dataset.addNode)))
  $$('[data-add-node]').forEach((button) => button.addEventListener('dragstart', beginPaletteDrag))
}

const nodeTemplates = {
  python: `import voxa\n\nclass MyNode:\n    def on_process(self, frame, input_port):\n        text = frame.text.upper()\n        return {"text_out": voxa.TextFrame(text, sequence=frame.sequence)}\n`,
  typescript: `import type { GraphFrame, GraphNodeImplementation } from '@voxa/core'\n\nexport const node: GraphNodeImplementation = {\n  onProcess(frame) {\n    return { text_out: { ...frame, text: frame.text.toUpperCase() } }\n  },\n}\n`,
  rust: `use voxa_core::{Node, NodeContext};\nuse voxa_types::Frame;\n\npub struct MyNode;\n\nimpl Node for MyNode {\n    fn on_process(&mut self, input: Option<Frame>, context: &mut NodeContext) -> voxa_types::Result<()> {\n        // Emit a derived Frame through text_out.\n        Ok(())\n    }\n}\n`,
  cpp: `#include <voxa/voxa.hpp>\n\nclass MyNode final : public voxa::MultimodalGraphNode {\n public:\n  std::vector<voxa::GraphEmission> on_process(\n      const voxa_frame_view_v1* input, std::string_view input_port) override {\n    return {};\n  }\n};\n`,
}
const defaultPorts = JSON.stringify([
  { name: 'text_in', direction: 'input', frame_type: 'text' },
  { name: 'text_out', direction: 'output', frame_type: 'text' },
], null, 2)

function openNodeLab() {
  $('#node-lab-ports').value = defaultPorts
  $('#node-lab-error').textContent = ''
  applyNodeTemplate()
  $('#node-lab-dialog').showModal()
}
function closeNodeLab() { $('#node-lab-dialog').close() }
function applyNodeTemplate() {
  const language = $('#node-lab-language').value
  $('#node-lab-code').value = nodeTemplates[language]
  const documentName = language === 'rust' ? 'rust' : language
  $('#node-lab-docs').href = `https://piyotahu.github.io/Voxa/nodes/${documentName}/`
  $('#node-lab-docs').textContent = `Open ${language === 'cpp' ? 'C++' : language[0].toUpperCase() + language.slice(1)} Node guide ↗`
  $('#node-lab-runtime-note').textContent = language === 'python' ? 'Text Python Nodes load only when you Run the Graph; saving never executes code.' : `${language} is registered for authoring; Studio will report its build Host requirements.`
}
async function saveNodePackage(event) {
  event.preventDefault()
  let ports, configSchema
  try { ports = JSON.parse($('#node-lab-ports').value); configSchema = JSON.parse($('#node-lab-schema').value) }
  catch (error) { $('#node-lab-error').textContent = error.message; return }
  const language = $('#node-lab-language').value
  const payload = {
    format: 'voxa.node/v1', package_id: $('#node-lab-package').value.trim(), display_name: $('#node-lab-display').value.trim(),
    node_type: $('#node-lab-type').value.trim(), language, factory_version: '1.0.0', kind: $('#node-lab-kind').value,
    entrypoint: language === 'python' ? 'node:MyNode' : language === 'typescript' ? 'node:node' : language === 'rust' ? 'node::MyNode' : 'MyNode',
    ports, config_schema: configSchema, code: $('#node-lab-code').value, runtime_available: false,
  }
  try {
    await api('/api/v1/node-library', { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(payload) })
    nodePackages = await api('/api/v1/node-library')
    const registrations = await api('/api/v1/registry/nodes')
    catalog.clear(); installCatalog([...registrations, ...nodePackages.map(packageCatalogEntry)]); renderPalette(); bindPaletteEvents()
    closeNodeLab(); toast(`${payload.display_name} registered in this project`)
  } catch (error) { $('#node-lab-error').textContent = error.message }
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
    const info = nodeInfo(node)
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

function beginPaletteDrag(event) {
  event.dataTransfer.effectAllowed = 'copy'
  event.dataTransfer.setData('application/x-voxa-node-factory', event.currentTarget.dataset.addNode)
}
function dropPaletteNode(event) {
  event.preventDefault()
  const key = event.dataTransfer.getData('application/x-voxa-node-factory')
  if (!key) return
  const point = canvasPoint(event.clientX, event.clientY)
  addNode(key, { x: point.x - 110, y: point.y - 70 })
}

function addNode(key, position = null) {
  const info = catalog.get(key)
  if (!info) return toast('Node Factory is no longer registered', true)
  if (info.runtime_available === false) return toast(`${info.display_name || info.node_type} is saved; activate its ${info.language} execution Host before adding it to a runnable Graph`, true)
  const base = info.kind === 'source' ? 'source' : info.kind === 'sink' ? 'sink' : 'transform'
  let number = 1
  while (state.graph.nodes.some((node) => node.id === `${base}-${number}`)) number++
  const id = `${base}-${number}`
  mutate(() => {
    state.graph.nodes.push({
      id, node_type: info.node_type, language: info.language, factory_version: info.factory_version,
      node_config: defaultConfig(info),
    })
    const sameKind = state.graph.nodes.filter((node) => nodeInfo(node).kind === info.kind).length - 1
    const x = info.kind === 'source' ? 110 : info.kind === 'sink' ? 840 : 475
    state.positions[id] = position || { x, y: 130 + sameKind * 175 }
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
  const selectedFactory = catalog.get($('#node-type').value)
  if (!selectedFactory) { $('#config-error').textContent = 'Select a registered Node Factory'; return }
  if (node.id === nextId && factoryKey(node) === $('#node-type').value && JSON.stringify(node.node_config) === JSON.stringify(config)) return
  const previousId = node.id
  mutate(() => {
    node.id = nextId
    node.node_type = selectedFactory.node_type
    node.language = selectedFactory.language
    node.factory_version = selectedFactory.factory_version
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

function openEdgeDialog(edgeId = null, preset = null) {
  const edge = edgeId ? state.graph.edges.find((candidate) => candidate.id === edgeId) : null
  state.editingEdge = edge?.id || null
  const sources = state.graph.nodes.filter((node) => nodeInfo(node).outputs.length)
  const targets = state.graph.nodes.filter((node) => nodeInfo(node).inputs.length)
  fillSelect($('#edge-from-node'), sources)
  fillSelect($('#edge-to-node'), targets)
  if (edge || preset) {
    $('#edge-from-node').value = (edge || preset).from.node_id
    $('#edge-to-node').value = (edge || preset).to.node_id
  }
  refreshEdgePorts()
  if (edge || preset) {
    $('#edge-from-port').value = (edge || preset).from.port
    refreshCompatibleInputPorts()
    $('#edge-to-port').value = (edge || preset).to.port
  }
  $('#edge-capacity').value = edge?.queue_policy?.capacity || 32
  $('#edge-overflow').value = edge?.queue_policy?.overflow || 'block'
  $('#edge-dialog-title').textContent = edge ? 'Edit edge' : 'Add edge'
  $('#edge-submit').textContent = edge ? 'Save edge' : 'Create edge'
  $('#edge-dialog').showModal()
}
function fillSelect(select, nodes) {
  select.replaceChildren(...nodes.map((node) => { const option = document.createElement('option'); option.value = node.id; option.textContent = node.id; return option }))
}
function refreshEdgePorts() {
  const source = state.graph.nodes.find((node) => node.id === $('#edge-from-node').value)
  fillStringSelect($('#edge-from-port'), source ? nodeInfo(source).outputs : [])
  refreshCompatibleInputPorts()
}
function portInfo(node, direction, name) {
  return nodeInfo(node)[direction === 'input' ? 'inputPorts' : 'outputPorts'].find((port) => port.name === name)
}
function refreshCompatibleInputPorts() {
  const source = state.graph.nodes.find((node) => node.id === $('#edge-from-node').value)
  const target = state.graph.nodes.find((node) => node.id === $('#edge-to-node').value)
  const output = source ? portInfo(source, 'output', $('#edge-from-port').value) : null
  const inputs = target ? nodeInfo(target).inputPorts.filter((port) => !output || port.frame_type === output.frame_type) : []
  fillStringSelect($('#edge-to-port'), inputs.map((port) => port.name))
}
function fillStringSelect(select, values) {
  select.replaceChildren(...values.map((value) => { const option = document.createElement('option'); option.value = value; option.textContent = value; return option }))
}
function submitEdge(event) {
  if (event.submitter?.value === 'cancel') return
  event.preventDefault()
  const from = $('#edge-from-node').value, to = $('#edge-to-node').value
  if (!from || !to) return toast('Add compatible source and target nodes first', true)
  const source = state.graph.nodes.find((node) => node.id === from)
  const target = state.graph.nodes.find((node) => node.id === to)
  const output = source && portInfo(source, 'output', $('#edge-from-port').value)
  const input = target && portInfo(target, 'input', $('#edge-to-port').value)
  if (!output || !input || output.frame_type !== input.frame_type) return toast('Choose ports with the same Frame type', true)
  let base = `${from}-${to}`, id = base, number = 2
  while (state.graph.edges.some((edge) => edge.id === id && edge.id !== state.editingEdge)) id = `${base}-${number++}`
  const next = {
    id, from: { node_id: from, port: $('#edge-from-port').value }, to: { node_id: to, port: $('#edge-to-port').value },
    frame_type: output.frame_type, queue_policy: { capacity: Number($('#edge-capacity').value), overflow: $('#edge-overflow').value },
  }
  mutate(() => {
    const index = state.graph.edges.findIndex((edge) => edge.id === state.editingEdge)
    if (index === -1) state.graph.edges.push(next)
    else state.graph.edges[index] = next
  })
  state.editingEdge = null
  $('#edge-dialog').close()
}
function deleteEdge(id) { mutate(() => { state.graph.edges = state.graph.edges.filter((edge) => edge.id !== id) }) }

function renderAll() {
  $('#graph-id').value = state.graph.graph_id
  $('#raw-json').value = JSON.stringify(state.graph, null, 2)
  $('#undo').disabled = !state.history.length
  $('#redo').disabled = !state.future.length
  renderCanvas(); renderEdgesList(); renderInspector(); renderRuntime()
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
    const source = state.graph.nodes.find((node) => node.id === edge.from.node_id)
    const target = state.graph.nodes.find((node) => node.id === edge.to.node_id)
    const x1 = from.x + 220, y1 = from.y + portY(source, 'output', edge.from.port), x2 = to.x, y2 = to.y + portY(target, 'input', edge.to.port)
    const metrics = (state.runtime.edges || []).find((value) => value.edge_id === edge.id)
    const runtimeClass = metrics?.drop_total ? ' runtime-drop' : metrics?.enqueue_total ? ' runtime-flow' : ''
    const path = svg('path', { d: edgePath(x1, y1, x2, y2), class: `graph-edge${runtimeClass}`, 'data-edge': edge.id })
    path.addEventListener('click', (event) => { event.stopPropagation(); openEdgeDialog(edge.id) })
    edgeLayer.append(path)
  }
  if (state.connection) {
    const { from, point } = state.connection
    const position = state.positions[from.node_id]
    const node = state.graph.nodes.find((candidate) => candidate.id === from.node_id)
    if (position && node) {
      const x1 = position.x + 220, y1 = position.y + portY(node, 'output', from.port)
      edgeLayer.append(svg('path', { d: edgePath(x1, y1, point.x, point.y), class: 'graph-edge connecting' }))
    }
  }
}

function edgePath(x1, y1, x2, y2) {
  const bend = Math.max(60, Math.abs(x2 - x1) * .48)
  return `M${x1},${y1} C${x1 + bend},${y1} ${x2 - bend},${y2} ${x2},${y2}`
}
function portY(node, direction, name) {
  if (!node) return 116
  const ports = nodeInfo(node)[direction === 'input' ? 'inputPorts' : 'outputPorts']
  return 116 + Math.max(0, ports.findIndex((port) => port.name === name)) * 20
}
function nodeHeight(info) { return Math.max(146, 128 + Math.max(info.inputs.length, info.outputs.length) * 20) }

function renderNode(node) {
  const info = nodeInfo(node)
  const position = state.positions[node.id] || { x: 100, y: 100 }
  const metrics = (state.runtime.nodes || []).find((value) => value.node_id === node.id)
  const active = (state.runtime.active_nodes || []).includes(node.id) ? ' runtime-active' : ''
  const failed = metrics?.error_total ? ' runtime-error' : ''
  const group = svg('g', { class: `node-group${node.id === state.selected ? ' selected' : ''}${active}${failed}`, transform: `translate(${position.x} ${position.y})`, 'data-node': node.id, tabindex: '0' })
  const height = nodeHeight(info)
  group.append(svg('rect', { class: 'node-card', width: 220, height, rx: 11 }))
  group.append(svg('rect', { class: `node-accent ${info.kind}`, width: 4, height: height - 30, x: 0, y: 15, rx: 2 }))
  addText(group, 19, 27, info.kind.toUpperCase(), 'node-kind-label')
  addText(group, 19, 51, node.id, 'node-title')
  addText(group, 19, 72, node.node_type, 'node-type-label')
  addText(group, 19, 94, `${info.language} · ${info.frameTypes.join('/') || 'control'}`, 'node-type-label')
  if (metrics) { const runtime = addText(group, 211, 94, `${metrics.process_total} calls · ${formatNanos(metrics.max_callback_duration_ns)} max`, 'node-runtime-label'); runtime.setAttribute('text-anchor', 'end') }
  info.inputPorts.forEach((port, index) => {
    const dot = svg('circle', { cx: 0, cy: 116 + index * 20, r: 6, class: 'port-dot input', 'data-node': node.id, 'data-port': port.name, 'data-frame-type': port.frame_type })
    group.append(dot); addText(group, 9, 120 + index * 20, `${port.name} · ${port.frame_type}`, 'port-label')
  })
  info.outputPorts.forEach((port, index) => {
    const dot = svg('circle', { cx: 220, cy: 116 + index * 20, r: 6, class: 'port-dot output', 'data-node': node.id, 'data-port': port.name, 'data-frame-type': port.frame_type })
    dot.addEventListener('pointerdown', beginConnection)
    group.append(dot); const text = addText(group, 211, 120 + index * 20, `${port.name} · ${port.frame_type}`, 'port-label'); text.setAttribute('text-anchor', 'end')
  })
  group.addEventListener('click', (event) => { event.stopPropagation(); selectNode(node.id) })
  group.addEventListener('pointerdown', (event) => beginDrag(event, node.id))
  return group
}
function addText(parent, x, y, value, className) { const text = svg('text', { x, y, class: className }); text.textContent = value; parent.append(text); return text }

function canvasPoint(clientX, clientY) {
  const canvas = $('#graph-canvas')
  const point = canvas.createSVGPoint(); point.x = clientX; point.y = clientY
  const matrix = canvas.getScreenCTM()
  return matrix ? point.matrixTransform(matrix.inverse()) : { x: clientX, y: clientY }
}

function beginConnection(event) {
  if (event.button !== 0) return
  event.preventDefault(); event.stopPropagation()
  const dot = event.currentTarget
  state.connection = {
    from: { node_id: dot.dataset.node, port: dot.dataset.port },
    frameType: dot.dataset.frameType,
    point: canvasPoint(event.clientX, event.clientY),
  }
  dot.setPointerCapture(event.pointerId)
  highlightCompatiblePorts(state.connection.frameType)
  const move = (next) => { state.connection.point = canvasPoint(next.clientX, next.clientY); renderEdgeLayer() }
  const stop = (next) => {
    const target = document.elementFromPoint(next.clientX, next.clientY)?.closest?.('.port-dot.input')
    const connection = state.connection
    dot.removeEventListener('pointermove', move); dot.removeEventListener('pointerup', stop); dot.removeEventListener('pointercancel', cancel)
    if (dot.hasPointerCapture(event.pointerId)) dot.releasePointerCapture(event.pointerId)
    state.connection = null; highlightCompatiblePorts(null)
    if (target && target.dataset.frameType === connection.frameType) createConnectedEdge(connection.from, { node_id: target.dataset.node, port: target.dataset.port }, connection.frameType)
    else renderEdgeLayer()
  }
  const cancel = () => {
    dot.removeEventListener('pointermove', move); dot.removeEventListener('pointerup', stop); dot.removeEventListener('pointercancel', cancel)
    state.connection = null; highlightCompatiblePorts(null); renderEdgeLayer()
  }
  dot.addEventListener('pointermove', move); dot.addEventListener('pointerup', stop); dot.addEventListener('pointercancel', cancel)
  renderEdgeLayer()
}
function highlightCompatiblePorts(frameType) {
  $$('.port-dot.input').forEach((dot) => dot.classList.toggle('compatible', Boolean(frameType) && dot.dataset.frameType === frameType))
}
function createConnectedEdge(from, to, frameType) {
  if (from.node_id === to.node_id) return toast('An Edge cannot connect a Node to itself', true)
  const duplicate = state.graph.edges.some((edge) => edge.from.node_id === from.node_id && edge.from.port === from.port && edge.to.node_id === to.node_id && edge.to.port === to.port)
  if (duplicate) return toast('That port connection already exists', true)
  let base = `${from.node_id}-${to.node_id}`, id = base, number = 2
  while (state.graph.edges.some((edge) => edge.id === id)) id = `${base}-${number++}`
  mutate(() => state.graph.edges.push({ id, from, to, frame_type: frameType, queue_policy: { capacity: 32, overflow: 'block' } }))
  toast(`${from.node_id}.${from.port} → ${to.node_id}.${to.port}`)
}

function beginDrag(event, id) {
  if (event.button !== 0 || event.target.closest('.port-dot')) return
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
    const route = document.createElement('div'); route.className = 'edge-route'; route.title = 'Click to edit this Edge'; route.addEventListener('click', () => openEdgeDialog(edge.id))
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
  $('#node-id').value = node.id; $('#node-type').value = factoryKey(node); $('#node-language').value = node.language; $('#node-version').value = node.factory_version || ''; $('#node-config').value = JSON.stringify(node.node_config, null, 2); $('#config-error').textContent = ''
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

function runtimeIsActive() { return ['starting', 'running', 'stopping', 'finishing'].includes(state.runtime.status) }
function scheduleRuntimePoll() {
  clearTimeout(state.runtimeTimer)
  state.runtimeTimer = setTimeout(refreshRuntime, runtimeIsActive() ? 350 : 1500)
}
async function refreshRuntime() {
  try {
    state.runtime = await api('/api/v1/runtime')
    renderRuntime(); renderCanvas()
  } catch (error) {
    toast(`Runtime metrics unavailable: ${error.message}`, true)
  }
  scheduleRuntimePoll()
}
async function startRuntime() {
  if (runtimeIsActive()) return
  if (!await validateGraph(false)) return toast('Fix validation errors before running', true)
  state.runtime = { status: 'starting', nodes: [], edges: [], active_nodes: [] }
  renderRuntime()
  try {
    state.runtime = await api('/api/v1/runtime/start', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(state.graph) })
    toast('Graph runtime started')
    renderRuntime(); renderCanvas(); scheduleRuntimePoll()
  } catch (error) {
    state.runtime = { status: 'idle', nodes: [], edges: [] }
    toast(error.status === 409 ? 'A graph run is already active' : error.message, true)
    renderRuntime()
  }
}
async function stopRuntime() {
  if (!runtimeIsActive()) return
  $('#stop').disabled = true
  try {
    state.runtime = await api('/api/v1/runtime/stop', { method: 'POST' })
    toast(state.runtime.accepted ? 'Stop requested' : 'Runtime is already stopping')
    renderRuntime(); renderCanvas(); scheduleRuntimePoll()
  } catch (error) { toast(error.message, true); renderRuntime() }
}
function renderRuntime() {
  const runtime = state.runtime || { status: 'idle', nodes: [], edges: [] }
  const panel = $('#runtime-panel')
  panel.className = `runtime-panel ${runtime.status || 'idle'}`
  $('#runtime-status').textContent = runtime.status || 'idle'
  $('#runtime-elapsed').textContent = runtime.elapsed_ms === undefined ? '—' : formatDuration(runtime.elapsed_ms)
  $('#runtime-session').textContent = runtime.session_id == null ? 'No session' : `Session #${runtime.session_id} · ${runtime.runtime_state || runtime.status}`
  const nodes = runtime.nodes || [], edges = runtime.edges || []
  $('#metric-node-calls').textContent = nodes.reduce((total, node) => total + node.prepare_total + node.process_total + node.signal_total + node.finish_total + node.abort_total, 0)
  $('#metric-frames').textContent = edges.reduce((total, edge) => total + edge.enqueue_total, 0)
  $('#metric-queued').textContent = edges.reduce((total, edge) => total + edge.queue_len, 0)
  $('#metric-drops').textContent = edges.reduce((total, edge) => total + edge.drop_total, 0)
  $('#run').disabled = runtimeIsActive()
  $('#stop').disabled = !runtimeIsActive() || runtime.status === 'stopping'
  const edgeRows = edges.map((edge) => {
    const row = document.createElement('div'); row.className = 'runtime-edge'
    const label = document.createElement('b'); label.textContent = edge.edge_id
    const meter = document.createElement('span'); meter.className = 'runtime-edge-meter'
    const fill = document.createElement('i'); fill.style.width = `${Math.min(100, edge.queue_capacity ? edge.high_watermark / edge.queue_capacity * 100 : 0)}%`; meter.append(fill)
    const detail = document.createElement('small'); detail.textContent = `${edge.dequeue_total} out · ${edge.drop_total} drop`
    row.append(label, meter, detail); return row
  })
  if (edgeRows.length) $('#runtime-edges').replaceChildren(...edgeRows)
  else { const empty = document.createElement('p'); empty.textContent = runtime.status === 'idle' ? 'Run the graph to inspect live Edge metrics.' : 'This graph has no Edges.'; $('#runtime-edges').replaceChildren(empty) }
  const terminal = $('#runtime-terminal')
  if (runtime.terminal?.kind && runtime.terminal.kind !== 'success' && runtime.terminal.message) {
    terminal.textContent = `${runtime.terminal.code || 'VOXA-RUNTIME'} · ${runtime.terminal.message}`
    terminal.classList.remove('hidden')
  } else terminal.classList.add('hidden')
}
function formatDuration(milliseconds) {
  if (milliseconds < 1000) return `${milliseconds} ms`
  return `${(milliseconds / 1000).toFixed(milliseconds < 10000 ? 1 : 0)} s`
}
function formatNanos(nanos) {
  if (nanos < 1000) return `${nanos}ns`
  if (nanos < 1000000) return `${(nanos / 1000).toFixed(1)}µs`
  return `${(nanos / 1000000).toFixed(1)}ms`
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
