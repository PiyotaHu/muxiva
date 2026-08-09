'use strict'

const NS = 'http://www.w3.org/2000/svg'
const VIEWPORT_WIDTH = 1200
const VIEWPORT_HEIGHT = 760
const MIN_ZOOM = .4
const MAX_ZOOM = 2.5
const catalog = new Map()
let nodePackages = []
const state = {
  token: '', graph: null, selected: null, positions: {}, diagnostics: [],
  history: [], future: [], dirty: false, zoom: 1, validating: null,
  viewport: { x: VIEWPORT_WIDTH / 2, y: VIEWPORT_HEIGHT / 2 }, suppressCanvasClick: false,
  runtime: { status: 'idle', nodes: [], edges: [] }, runtimeTimer: null,
  telemetry: { previous: null, nodeRates: new Map(), edgeRates: new Map(), selection: null, history: { sessions: [], selected: null, samples: [], loaded: false, export: null } },
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
  if (fragment) sessionStorage.setItem('muxiva.studio.token', fragment)
  history.replaceState(null, '', location.pathname)
  return fragment || sessionStorage.getItem('muxiva.studio.token') || ''
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
    state.graph = migrateGraph(typeof graph === 'string' ? JSON.parse(graph) : graph)
    ingestRuntimeSnapshot(runtime)
    state.runtime = runtime
    nodePackages = packages
    installCatalog([...registrations, ...packages.map(packageCatalogEntry)])
    renderPalette()
    $('#graph-path').textContent = metadata.graph_path
    $('#connection-status').textContent = metadata.writable ? 'Local runtime · writable' : 'Local runtime · read only'
    if (metadata.project_demo) {
      $('#project-demo').classList.remove('hidden')
      $('#project-demo').addEventListener('click', async () => {
        if (state.dirty && !await saveGraph()) return
        window.open(`/project/index.html#${state.token}`, '_blank', 'noopener,noreferrer')
      })
    }
    seedPositions()
    bindEvents()
    renderAll()
    await validateGraph(false)
    scheduleRuntimePoll()
  } catch (error) {
    fatal(error.status === 401 ? 'The Studio access token is invalid or expired.' : error.message)
  }
}

function migrateGraph(graph) {
  const names = {
    'provider.agora.audio_source': 'agora.audio_source',
    'provider.agora.audio_sink': 'agora.audio_sink',
    'provider.qwen.audio_realtime': 'qwen.audio_realtime',
    'provider.qwen.asr_realtime': 'qwen.asr_realtime',
    'provider.qwen.llm_stream': 'qwen.llm_stream',
    'provider.qwen.tts_realtime': 'qwen.tts_realtime',
    'builtin.audio_resample': 'builtin.audio_resampler',
  }
  const oldSources = new Set(graph.nodes.filter(node => node.node_type === 'provider.agora.audio_source').map(node => node.id))
  graph.nodes.forEach(node => {
    if (node.node_type === 'provider.agora.audio_source') node.factory_version = '1.1.0'
    node.node_type = names[node.node_type] || node.node_type
  })
  graph.edges = graph.edges.filter(edge => !(oldSources.has(edge.to.node_id) && edge.to.port === 'tick_in'))
  const referenced = new Set(graph.edges.flatMap(edge => [edge.from.node_id, edge.to.node_id]))
  graph.nodes = graph.nodes.filter(node => node.node_type !== 'builtin.interval_tick' || referenced.has(node.id))
  return graph
}

function factoryKey(value) { return JSON.stringify([value.node_type, value.language, value.factory_version]) }
function packageCatalogEntry(value) { return { ...value, runtime_available: value.runtime_available } }
function nodeInfo(node) {
  return catalog.get(factoryKey(node)) || {
    kind: 'transform', category: 'utility', capability: 'unknown', label: node.node_type, language: node.language || 'unknown',
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
  const query = ($('#palette-search')?.value || '').trim().toLowerCase()
  const selectedCategory = $('#palette-category')?.value || 'all'
  const entries = [...catalog.entries()]
    .filter(([, entry]) => selectedCategory === 'all' || (entry.category || 'utility') === selectedCategory)
    .filter(([, entry]) => !query || [entry.display_name, entry.node_type, entry.capability, ...(entry.tags || [])].join(' ').toLowerCase().includes(query))
    .sort((left, right) => `${left[1].category}:${left[1].display_name || left[1].node_type}`.localeCompare(`${right[1].category}:${right[1].display_name || right[1].node_type}`))
  const layers = new Map()
  for (const item of entries) {
    const category = item[1].category || 'utility'
    if (!layers.has(category)) layers.set(category, [])
    layers.get(category).push(item)
  }
  const groups = [...layers.entries()].map(([category, items]) => {
    const group = document.createElement('section'); group.className = 'palette-group'
    const heading = document.createElement('div'); heading.className = `palette-group-heading ${category}`; heading.textContent = category
    const buttons = items.map(([key, entry]) => {
    const button = document.createElement('button'); button.className = `palette-item ${entry.kind}`; button.dataset.addNode = key; button.draggable = true
    const icon = document.createElement('span'); icon.className = `node-icon category-${entry.category || 'utility'}`; icon.textContent = (entry.category || 'utility')[0].toUpperCase()
    const copy = document.createElement('span'), label = document.createElement('b'), detail = document.createElement('small')
    label.textContent = entry.display_name || entry.label; detail.textContent = `${entry.capability || entry.node_type} · ${entry.language}`; copy.append(label, detail)
    const add = document.createElement('span'); add.textContent = '＋'; button.append(icon, copy, add); return button
    })
    group.append(heading, ...buttons); return group
  })
  if (!groups.length) { const empty = document.createElement('p'); empty.className = 'palette-empty'; empty.textContent = 'No Nodes match this filter.'; groups.push(empty) }
  $('#node-palette').replaceChildren(...groups)
  bindPaletteEvents()
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
  $('#palette-search').addEventListener('input', renderPalette)
  $('#palette-category').addEventListener('change', renderPalette)
  $('#graph-id').addEventListener('change', (event) => mutate(() => { state.graph.graph_id = event.target.value.trim() }))
  $('#node-id').addEventListener('change', updateSelectedNode)
  $('#node-type').addEventListener('change', updateSelectedNode)
  $('#node-config').addEventListener('change', updateSelectedNode)
  $('#node-config').addEventListener('blur', updateSelectedNode)
  $('#delete-node').addEventListener('click', deleteSelectedNode)
  $('#add-edge').addEventListener('click', () => openEdgeDialog())
  $('#open-node-lab').addEventListener('click', () => openNodeLab())
  $('#edit-node-code').addEventListener('click', editSelectedNodeCode)
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
  $('#providers').addEventListener('click', openProviders)
  $('#observability').addEventListener('click', () => openObservability())
  $('#observability-close').addEventListener('click', closeObservability)
  $('#observe-history-refresh').addEventListener('click', () => refreshObservabilityHistory(false))
  $('#provider-close').addEventListener('click', closeProviders)
  $('#provider-cancel').addEventListener('click', closeProviders)
  $('#provider-form').addEventListener('submit', saveProviders)
  $('#templates').addEventListener('click', openTemplates)
  $('#template-close').addEventListener('click', closeTemplates)
  $('#template-cancel').addEventListener('click', closeTemplates)
  $('#raw-close').addEventListener('click', closeRaw)
  $('#format-json').addEventListener('click', formatRaw)
  $('#apply-json').addEventListener('click', applyRaw)
  $('#zoom-in').addEventListener('click', () => setZoom(state.zoom + .1))
  $('#zoom-out').addEventListener('click', () => setZoom(state.zoom - .1))
  $('#fit-view').addEventListener('click', fitView)
  const canvas = $('#graph-canvas')
  canvas.addEventListener('click', (event) => {
    if (state.suppressCanvasClick) { state.suppressCanvasClick = false; return }
    if (event.target.id === 'graph-canvas') selectNode(null)
  })
  canvas.addEventListener('pointerdown', beginCanvasPan)
  canvas.addEventListener('wheel', zoomCanvasAtPointer, { passive: false })
  canvas.addEventListener('dragover', (event) => { event.preventDefault(); event.dataTransfer.dropEffect = 'copy' })
  canvas.addEventListener('drop', dropPaletteNode)
  window.addEventListener('keydown', keyboardShortcut)
}

async function openProviders() {
  $('#provider-error').textContent = ''
  try {
    const status = await api('/api/v1/connections')
    renderProviderStatus(status)
    $('#provider-dialog').showModal()
  } catch (error) { toast(error.message, true) }
}
function closeProviders() {
  $$('#provider-connections input[type="password"]').forEach((input) => { input.value = '' })
  $('#provider-dialog').close()
}
function renderProviderStatus(status) {
  const cards = (status.connections || []).map((connection) => {
    const card = document.createElement('section'); card.className = 'provider-card'; card.dataset.connection = connection.id
    const title = document.createElement('div'); title.className = 'provider-title'
    const copy = document.createElement('div'), name = document.createElement('b'), description = document.createElement('small')
    name.textContent = connection.display_name; description.textContent = connection.description; copy.append(name, description)
    const badge = document.createElement('span'); badge.className = `provider-badge${connection.configured ? ' ready' : ''}`; badge.textContent = connection.configured ? 'Ready' : 'Not configured'
    title.append(copy, badge); card.append(title)
    for (const field of connection.fields || []) {
      const label = document.createElement('label'); label.textContent = field.label
      const input = document.createElement('input'); input.dataset.connection = connection.id; input.dataset.field = field.name
      input.type = field.secret ? 'password' : 'text'; input.autocomplete = 'off'; input.spellcheck = false
      input.value = field.secret ? '' : field.value || ''
      input.placeholder = field.secret && field.set ? 'Saved in project .env · paste to replace' : field.required ? `Required · ${field.environment}` : 'Optional'
      label.append(input)
      if (field.help) { const help = document.createElement('small'); help.className = 'provider-help'; help.textContent = field.help; label.append(help) }
      if (field.acquire_url) { const link = document.createElement('a'); link.className = 'provider-acquire'; link.href = field.acquire_url; link.target = '_blank'; link.rel = 'noreferrer'; link.textContent = 'Get this value from the official console ↗'; label.append(link) }
      card.append(label)
    }
    return card
  })
  if (!cards.length) {
    const empty = document.createElement('p'); empty.className = 'dialog-copy'; empty.textContent = 'No installed Node Pack declares a connection.'; cards.push(empty)
  }
  $('#provider-connections').replaceChildren(...cards)
  $('#provider-storage').textContent = status.storage === 'project-.env'
    ? 'Local storage: project .env · Git ignored · file mode 0600.'
    : `Secret storage: ${status.storage}`
}
async function saveProviders(event) {
  event.preventDefault()
  const payload = { connections: {} }
  $$('#provider-connections input[data-connection]').forEach((input) => {
    if (input.type === 'password' && !input.value) return
    payload.connections[input.dataset.connection] ||= {}
    payload.connections[input.dataset.connection][input.dataset.field] = input.value
  })
  try {
    const status = await api('/api/v1/connections', { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(payload) })
    renderProviderStatus(status)
    toast('Connections saved to project .env. They will load automatically next time.')
  } catch (error) { $('#provider-error').textContent = error.message }
}

function templateAvailable(template) { return template.graph.nodes.every((node) => catalog.get(factoryKey(node))?.runtime_available !== false) }
async function openTemplates() {
  try {
    const templates = await api('/api/v1/templates')
    const cards = templates.map((template) => {
    const available = templateAvailable(template), card = document.createElement('article'); card.className = 'template-card'
    const title = document.createElement('div'); title.className = 'template-title'; title.innerHTML = `<div><b>${template.name}</b><small>${template.badge}</small></div><span>${template.graph.nodes.length} Nodes</span>`
    const description = document.createElement('p'); description.textContent = template.description
    const traits = document.createElement('ul'); traits.replaceChildren(...template.traits.map((value) => { const item = document.createElement('li'); item.textContent = value; return item }))
    const button = document.createElement('button'); button.className = 'button primary'; button.textContent = available ? 'Use this graph' : 'Official Nodes installing'; button.disabled = !available
    button.addEventListener('click', () => applyTemplate(template))
    card.append(title, description, traits, button); return card
    })
    if (!cards.length) { const empty = document.createElement('p'); empty.className = 'dialog-copy'; empty.textContent = 'No project template is installed under .muxiva/templates/.'; cards.push(empty) }
    $('#template-gallery').replaceChildren(...cards); $('#template-dialog').showModal()
  } catch (error) { toast(error.message, true) }
}
function closeTemplates() { $('#template-dialog').close() }
function applyTemplate(template) {
  mutate(() => { state.graph = clone(template.graph); state.selected = null; state.positions = {}; seedPositions() })
  fitView()
  closeTemplates(); toast(`${template.name} graph applied · Save graph to persist it`)
}

function bindPaletteEvents() {
  $$('[data-add-node]').forEach((button) => button.addEventListener('click', () => addNode(button.dataset.addNode)))
  $$('[data-add-node]').forEach((button) => button.addEventListener('dragstart', beginPaletteDrag))
}

const nodeTemplates = {
  python: `import muxiva\n\nclass MyNode:\n    def on_process(self, frame, ctx):\n        # Data stays on typed graph ports; no return value is required.\n        ctx.emit("text_out", muxiva.TextFrame(frame.text.upper(), sequence=frame.sequence))\n        # Low-frequency observers receive this outside the data path.\n        ctx.publish_notification("example.node.processed", {"sequence": frame.sequence})\n`,
  typescript: `import type { GraphNodeImplementation } from '@muxiva/core'\n\nexport const node: GraphNodeImplementation = {\n  onProcess(frame, ctx) {\n    ctx.emit('text_out', { ...frame, text: frame.text.toUpperCase() })\n    ctx.publishNotification('example.node.processed', { sequence: frame.sequence })\n  },\n}\n`,
  rust: `use muxiva_core::{Node, NodeContext};\nuse muxiva_types::Frame;\n\npub struct MyNode;\n\nimpl Node for MyNode {\n    fn on_process(&mut self, input: Option<Frame>, context: &mut NodeContext) -> muxiva_types::Result<()> {\n        // Emit a derived Frame through text_out.\n        Ok(())\n    }\n}\n`,
  cpp: `#include <muxiva/muxiva.hpp>\n\nclass MyNode final : public muxiva::MultimodalGraphNode {\n public:\n  void on_process(const muxiva_frame_view_v1* input,\n                  muxiva::GraphNodeContext& ctx) override {\n    // ctx.emit("text_out", output_frame);\n  }\n};\n`,
}
const defaultPorts = JSON.stringify([
  { name: 'text_in', direction: 'input', frame_type: 'text' },
  { name: 'text_out', direction: 'output', frame_type: 'text' },
], null, 2)

function openNodeLab(packageValue = null) {
  $('#node-lab-title').textContent = packageValue ? 'Edit Node Code' : 'Create a Node'
  $('#node-lab-language').disabled = Boolean(packageValue)
  if (packageValue) {
    $('#node-lab-language').value = packageValue.language
    $('#node-lab-kind').value = packageValue.kind
    $('#node-lab-category').value = packageValue.category || 'utility'
    $('#node-lab-capability').value = packageValue.capability || 'custom'
    $('#node-lab-summary').value = packageValue.summary || ''
    $('#node-lab-package').value = packageValue.package_id
    $('#node-lab-display').value = packageValue.display_name
    $('#node-lab-type').value = packageValue.node_type
    $('#node-lab-ports').value = JSON.stringify(packageValue.ports, null, 2)
    $('#node-lab-schema').value = JSON.stringify(packageValue.config_schema, null, 2)
    $('#node-lab-code').value = packageValue.code
  } else {
    $('#node-lab-language').disabled = false
    $('#node-lab-ports').value = defaultPorts
    applyNodeTemplate()
  }
  $('#node-lab-error').textContent = ''
  updateNodeLabHelp()
  $('#node-lab-dialog').showModal()
}
function closeNodeLab() { $('#node-lab-dialog').close() }
function applyNodeTemplate() {
  const language = $('#node-lab-language').value
  $('#node-lab-code').value = nodeTemplates[language]
  updateNodeLabHelp()
}
function updateNodeLabHelp() {
  const language = $('#node-lab-language').value
  const documentName = language === 'rust' ? 'rust' : language
  $('#node-lab-docs').href = `https://piyotahu.github.io/muxiva/nodes/${documentName}/`
  $('#node-lab-docs').textContent = `Open ${language === 'cpp' ? 'C++' : language[0].toUpperCase() + language.slice(1)} Node guide ↗`
  $('#node-lab-runtime-note').textContent = language === 'python' ? 'Text Python Nodes load only when you Run the Graph; saving never executes code.' : `${language} is registered for authoring; Studio will report its build Host requirements.`
}
function editSelectedNodeCode() {
  const node = selectedNode()
  const packageValue = node && nodePackages.find((candidate) => factoryKey(candidate) === factoryKey(node))
  if (packageValue?.editable) openNodeLab(packageValue)
}
async function saveNodePackage(event) {
  event.preventDefault()
  let ports, configSchema
  try { ports = JSON.parse($('#node-lab-ports').value); configSchema = JSON.parse($('#node-lab-schema').value) }
  catch (error) { $('#node-lab-error').textContent = error.message; return }
  const language = $('#node-lab-language').value
  const payload = {
    format: 'muxiva.node/v1', package_id: $('#node-lab-package').value.trim(), display_name: $('#node-lab-display').value.trim(),
    node_type: $('#node-lab-type').value.trim(), language, factory_version: '1.0.0', kind: $('#node-lab-kind').value,
    category: $('#node-lab-category').value, capability: $('#node-lab-capability').value.trim(), summary: $('#node-lab-summary').value.trim(),
    entrypoint: language === 'python' ? 'node:MyNode' : language === 'typescript' ? 'node:node' : language === 'rust' ? 'node::MyNode' : 'MyNode',
    ports, config_schema: configSchema, code: $('#node-lab-code').value, runtime_available: false,
  }
  try {
    await api('/api/v1/node-library', { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(payload) })
    nodePackages = await api('/api/v1/node-library')
    const registrations = await api('/api/v1/registry/nodes')
    catalog.clear(); installCatalog([...registrations, ...nodePackages.map(packageCatalogEntry)]); renderPalette()
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
  event.dataTransfer.setData('application/x-muxiva-node-factory', event.currentTarget.dataset.addNode)
}
function dropPaletteNode(event) {
  event.preventDefault()
  const key = event.dataTransfer.getData('application/x-muxiva-node-factory')
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
  applyViewport()
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

function viewportBox() {
  const width = VIEWPORT_WIDTH / state.zoom
  const height = VIEWPORT_HEIGHT / state.zoom
  return { x: state.viewport.x - width / 2, y: state.viewport.y - height / 2, width, height }
}

function applyViewport() {
  const box = viewportBox()
  $('#graph-canvas').setAttribute('viewBox', `${box.x} ${box.y} ${box.width} ${box.height}`)
  $('#zoom-label').textContent = `${Math.round(state.zoom * 100)}%`
}

function beginCanvasPan(event) {
  if (![0, 1].includes(event.button)) return
  if (event.target.closest?.('.node-group, .port-dot, .graph-edge')) return
  const canvas = event.currentTarget
  const matrix = canvas.getScreenCTM()
  if (!matrix) return
  event.preventDefault()
  canvas.setPointerCapture(event.pointerId)
  canvas.classList.add('panning')
  canvas.style.cursor = 'grabbing'
  const inverse = matrix.inverse()
  const startPoint = canvas.createSVGPoint()
  startPoint.x = event.clientX
  startPoint.y = event.clientY
  const origin = startPoint.matrixTransform(inverse)
  const center = { ...state.viewport }
  let moved = false
  const move = (next) => {
    const screenPoint = canvas.createSVGPoint()
    screenPoint.x = next.clientX
    screenPoint.y = next.clientY
    const current = screenPoint.matrixTransform(inverse)
    const deltaX = current.x - origin.x
    const deltaY = current.y - origin.y
    if (Math.abs(deltaX) > 2 || Math.abs(deltaY) > 2) moved = true
    state.viewport = { x: center.x - deltaX, y: center.y - deltaY }
    applyViewport()
  }
  const stop = () => {
    canvas.removeEventListener('pointermove', move)
    canvas.removeEventListener('pointerup', stop)
    canvas.removeEventListener('pointercancel', stop)
    if (canvas.hasPointerCapture(event.pointerId)) canvas.releasePointerCapture(event.pointerId)
    canvas.classList.remove('panning')
    canvas.style.cursor = 'grab'
    state.suppressCanvasClick = moved
  }
  canvas.addEventListener('pointermove', move)
  canvas.addEventListener('pointerup', stop)
  canvas.addEventListener('pointercancel', stop)
}

function zoomCanvasAtPointer(event) {
  event.preventDefault()
  const anchor = canvasPoint(event.clientX, event.clientY)
  const factor = Math.exp(-event.deltaY * .0015)
  setZoom(state.zoom * factor, { clientX: event.clientX, clientY: event.clientY, anchor })
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
  const startPoint = canvasPoint(event.clientX, event.clientY)
  const start = { point: startPoint, nodeX: state.positions[id].x, nodeY: state.positions[id].y }
  const move = (next) => {
    const point = canvasPoint(next.clientX, next.clientY)
    state.positions[id] = { x: start.nodeX + point.x - start.point.x, y: start.nodeY + point.y - start.point.y }
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
  const info = nodeInfo(node)
  $('#node-category').textContent = info.category || 'utility'
  $('#node-capability').textContent = info.capability || 'unknown'
  $('#node-summary').textContent = info.summary || 'No summary is declared for this Node.'
  $('#node-port-contracts').textContent = (info.ports || []).map((port) => {
    const contract = port.schema && Object.keys(port.schema).length ? ` ${JSON.stringify(port.schema)}` : ''
    return `${port.direction === 'input' ? '←' : '→'} ${port.name}: ${port.frame_type}${contract}`
  }).join('\n') || 'No data ports · control-only Node'
  const documentation = $('#node-documentation')
  documentation.classList.toggle('hidden', !info.documentation)
  if (info.documentation) documentation.href = info.documentation
  const projectPackage = nodePackages.find((candidate) => factoryKey(candidate) === factoryKey(node))
  const code = $('#node-source-code'), meta = $('#node-source-meta'), edit = $('#edit-node-code'), link = $('#node-source-link')
  edit.classList.toggle('hidden', !projectPackage?.editable)
  link.classList.toggle('hidden', Boolean(projectPackage) || node.language !== 'rust' || !node.node_type.startsWith('builtin.'))
  if (projectPackage) {
    const location = projectPackage.origin === 'provider' ? `official Nodes · ${projectPackage.package_id} · shared read-only source` : `.muxiva/nodes/${projectPackage.package_id}/ · exact project source`
    meta.textContent = `${projectPackage.language} · ${location}`
    code.value = projectPackage.code
  } else if (node.language === 'rust' && node.node_type.startsWith('builtin.')) {
    meta.textContent = `Rust · ${node.node_type} is compiled into Muxiva; open the authoritative source below.`
    code.value = `// ${node.node_type}\n// This built-in is compiled into the Muxiva binary.\n// Use the source link below to inspect its authoritative Node implementation.`
  } else {
    meta.textContent = `${info.language || node.language} · no source package is installed in this project`
    code.value = '// Source is unavailable. Register this factory as a project Node package to make it inspectable.'
  }
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
    state.diagnostics = Array.isArray(error.data) ? error.data : [{ code: 'MUXIVA-STUDIO', pointer: '', message: error.message }]
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
    const previousStatus = state.runtime?.status
    const runtime = await api('/api/v1/runtime')
    ingestRuntimeSnapshot(runtime)
    state.runtime = runtime
    renderRuntime(); renderCanvas()
    if (['completed', 'aborted', 'stopped'].includes(runtime.status) && runtime.status !== previousStatus) refreshObservabilityHistory(true)
  } catch (error) {
    toast(`Runtime metrics unavailable: ${error.message}`, true)
  }
  scheduleRuntimePoll()
}
async function startRuntime() {
  if (runtimeIsActive()) return
  if (!await validateGraph(false)) return toast('Fix validation errors before running', true)
  state.telemetry = { previous: null, nodeRates: new Map(), edgeRates: new Map(), selection: null, history: state.telemetry.history }
  state.runtime = { status: 'starting', nodes: [], edges: [], active_nodes: [] }
  renderRuntime()
  try {
    const runtime = await api('/api/v1/runtime/start', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(state.graph) })
    ingestRuntimeSnapshot(runtime)
    state.runtime = runtime
    toast('Graph runtime started')
    renderRuntime(); renderCanvas(); scheduleRuntimePoll()
  } catch (error) {
    state.runtime = { status: 'idle', nodes: [], edges: [] }
    if (error.status === 412) { toast(error.message, true); await openProviders() }
    else toast(error.status === 409 ? 'A graph run is already active' : error.message, true)
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
    row.title = 'Open Edge telemetry'
    row.addEventListener('click', () => openObservability('edge', edge.edge_id))
    row.append(label, meter, detail); return row
  })
  if (edgeRows.length) $('#runtime-edges').replaceChildren(...edgeRows)
  else { const empty = document.createElement('p'); empty.textContent = runtime.status === 'idle' ? 'Run the graph to inspect live Edge metrics.' : 'This graph has no Edges.'; $('#runtime-edges').replaceChildren(empty) }
  const terminal = $('#runtime-terminal')
  if (runtime.terminal?.kind && runtime.terminal.kind !== 'success' && runtime.terminal.message) {
    terminal.textContent = `${runtime.terminal.code || 'MUXIVA-RUNTIME'} · ${runtime.terminal.message}`
    terminal.classList.remove('hidden')
  } else terminal.classList.add('hidden')
  if (!$('#observability-page').classList.contains('hidden')) renderObservability()
}

function ingestRuntimeSnapshot(next) {
  const previous = state.telemetry.previous
  const sameSession = previous?.session_id != null && previous.session_id === next?.session_id
  const elapsedSeconds = sameSession ? Math.max(.001, ((next.elapsed_ms || 0) - (previous.elapsed_ms || 0)) / 1000) : 0
  const previousNodes = new Map((previous?.nodes || []).map((node) => [node.node_id, node]))
  const previousEdges = new Map((previous?.edges || []).map((edge) => [edge.edge_id, edge]))
  state.telemetry.nodeRates = new Map((next?.nodes || []).map((node) => {
    const old = previousNodes.get(node.node_id)
    return [node.node_id, {
      process: elapsedSeconds && old ? positiveDelta(node.process_total, old.process_total) / elapsedSeconds : 0,
      processDuration: elapsedSeconds && old ? positiveDelta(node.process_duration_ns, old.process_duration_ns) : 0,
    }]
  }))
  state.telemetry.edgeRates = new Map((next?.edges || []).map((edge) => {
    const old = previousEdges.get(edge.edge_id)
    const frames = elapsedSeconds && old ? positiveDelta(edge.enqueue_total, old.enqueue_total) / elapsedSeconds : 0
    const audioNs = elapsedSeconds && old ? positiveDelta(edge.audio_duration_ns_total, old.audio_duration_ns_total) : 0
    return [edge.edge_id, {
      frames,
      bytes: elapsedSeconds && old ? positiveDelta(edge.payload_bytes_total, old.payload_bytes_total) / elapsedSeconds : 0,
      blockedNs: old ? positiveDelta(edge.blocked_duration_ns, old.blocked_duration_ns) : 0,
      drops: old ? positiveDelta(edge.drop_total, old.drop_total) : 0,
      mediaRatio: elapsedSeconds ? audioNs / (elapsedSeconds * 1e9) : 0,
    }]
  }))
  state.telemetry.previous = clone(next || {})
}
function positiveDelta(current = 0, previous = 0) { return Math.max(0, Number(current) - Number(previous)) }
function customMetric(node, name) { return (node.custom_metrics || []).find((metric) => metric.name === name)?.value }
function graphEdge(edgeId) { return state.graph?.edges?.find((edge) => edge.id === edgeId) }
function graphNode(nodeId) { return state.graph?.nodes?.find((node) => node.id === nodeId) }
function edgeRoute(edgeId) {
  const edge = graphEdge(edgeId)
  return edge ? `${edge.from.node_id}.${edge.from.port} → ${edge.to.node_id}.${edge.to.port}` : edgeId
}
function severityRank(value) { return { idle: 0, healthy: 1, warning: 2, critical: 3 }[value] || 0 }
function worstSeverity(...values) { return values.reduce((worst, value) => severityRank(value) > severityRank(worst) ? value : worst, 'healthy') }
function edgeVerdict(edge) {
  const rate = state.telemetry.edgeRates.get(edge.edge_id) || {}
  const ratio = edge.queue_capacity ? edge.queue_len / edge.queue_capacity : 0
  const ageMs = (edge.oldest_frame_age_ns || 0) / 1e6
  if (edge.latest_error_reason || edge.drop_total > 0 || ratio >= .8 || ageMs >= 1000) return { severity: 'critical', reason: edge.latest_error_reason || (edge.drop_total > 0 ? `${edge.drop_total} frame(s) dropped` : ratio >= .8 ? `queue ${Math.round(ratio * 100)}% full` : `oldest frame waiting ${formatDuration(ageMs)}`) }
  if (ratio >= .4 || ageMs >= 200 || edge.full_total > 0 || (rate.blockedNs || 0) > 0) return { severity: 'warning', reason: ratio >= .4 ? `queue ${Math.round(ratio * 100)}% full` : ageMs >= 200 ? `oldest frame waiting ${formatDuration(ageMs)}` : 'producer was blocked by backpressure' }
  return { severity: 'healthy', reason: edge.enqueue_total ? 'flowing without visible pressure' : 'no frames observed yet' }
}
function nodeVerdict(node) {
  const averageMs = node.process_total ? node.process_duration_ns / node.process_total / 1e6 : 0
  const queueMs = Number(customMetric(node, 'ingress.queue_duration_ms') || 0)
  const ingressDrops = Number(customMetric(node, 'ingress.dropped_frames') || 0)
  const connected = (state.runtime.edges || []).filter((edge) => {
    const graph = graphEdge(edge.edge_id)
    return graph && (graph.from.node_id === node.node_id || graph.to.node_id === node.node_id)
  }).map((edge) => edgeVerdict(edge).severity)
  let own = 'healthy', reason = node.process_total ? 'callbacks are within thresholds' : 'waiting for input'
  if (node.error_total || node.panic_total || ingressDrops > 0 || queueMs >= 1000 || averageMs >= 50) {
    own = 'critical'; reason = node.error_total || node.panic_total ? `${node.error_total + node.panic_total} callback failure(s)` : ingressDrops > 0 ? `${ingressDrops} ingress frame(s) dropped` : queueMs >= 1000 ? `internal ingress queue is ${formatDuration(queueMs)}` : `average process time is ${averageMs.toFixed(1)}ms`
  } else if (queueMs >= 200 || averageMs >= 10 || node.max_process_duration_ns / 1e6 >= 100) {
    own = 'warning'; reason = queueMs >= 200 ? `internal ingress queue is ${formatDuration(queueMs)}` : `slow callback: avg ${averageMs.toFixed(1)}ms, max ${(node.max_process_duration_ns / 1e6).toFixed(1)}ms`
  }
  const severity = worstSeverity(own, ...connected)
  if (severity !== own) reason = 'a connected Edge is under backpressure'
  return { severity, reason }
}
function overallHealth() {
  if (!state.runtime?.session_id) return 'idle'
  return [...(state.runtime.nodes || []).map((node) => nodeVerdict(node).severity), ...(state.runtime.edges || []).map((edge) => edgeVerdict(edge).severity)].reduce(worstSeverity, 'healthy')
}
function openObservability(kind = null, id = null) {
  if (kind && id) state.telemetry.selection = { kind, id }
  $('#observability-page').classList.remove('hidden')
  $('#observability-page').setAttribute('aria-hidden', 'false')
  renderObservability()
  refreshObservabilityHistory(true)
}
function closeObservability() {
  $('#observability-page').classList.add('hidden')
  $('#observability-page').setAttribute('aria-hidden', 'true')
}
function renderObservability() {
  const runtime = state.runtime || { nodes: [], edges: [] }, nodes = runtime.nodes || [], edges = runtime.edges || []
  const nodeRate = nodes.reduce((total, node) => total + (state.telemetry.nodeRates.get(node.node_id)?.process || 0), 0)
  const frameRate = edges.reduce((total, edge) => total + (state.telemetry.edgeRates.get(edge.edge_id)?.frames || 0), 0)
  const queued = edges.reduce((total, edge) => total + edge.queue_len, 0), capacity = edges.reduce((total, edge) => total + edge.queue_capacity, 0)
  const verdicts = [...nodes.map((node) => ({ kind: 'node', id: node.node_id, ...nodeVerdict(node) })), ...edges.map((edge) => ({ kind: 'edge', id: edge.edge_id, ...edgeVerdict(edge) }))]
  const unhealthy = verdicts.filter((value) => severityRank(value.severity) >= severityRank('warning'))
  const health = overallHealth(), badge = $('#observe-health')
  badge.className = `health-badge ${health}`; badge.textContent = health.toUpperCase()
  $('#observe-session').textContent = runtime.session_id == null ? 'No runtime session · press Run to collect telemetry' : `Session #${runtime.session_id} · ${runtime.status} · ${formatDuration(runtime.elapsed_ms || 0)}`
  $('#observe-node-rate').textContent = `${formatRate(nodeRate)}/s`; $('#observe-frame-rate').textContent = `${formatRate(frameRate)}/s`
  $('#observe-queued').textContent = queued; $('#observe-queue-detail').textContent = capacity ? `${Math.round(queued / capacity * 100)}% of ${capacity} slots` : 'No Edge queues'
  $('#observe-bottlenecks').textContent = unhealthy.length
  renderHotspots(unhealthy); renderObservationHistory(); renderObserveNodes(nodes); renderObserveEdges(edges); renderObserveDetail()
}
function renderHotspots(unhealthy) {
  if (!unhealthy.length) { const empty = document.createElement('div'); empty.className = 'hotspot-empty'; empty.textContent = state.runtime?.session_id ? '✓ No bottleneck detected in this session.' : 'Run the Graph to begin bottleneck detection.'; $('#observe-hotspots').replaceChildren(empty); return }
  const values = unhealthy.sort((a, b) => severityRank(b.severity) - severityRank(a.severity)).slice(0, 6)
  $('#observe-hotspots').replaceChildren(...values.map((value) => {
    const item = document.createElement('div'); item.className = `hotspot ${value.severity}`
    const dot = document.createElement('i'), copy = document.createElement('div'), title = document.createElement('b'), reason = document.createElement('small')
    title.textContent = `${value.kind === 'node' ? 'Node' : 'Edge'} · ${value.id}`; reason.textContent = value.reason; copy.append(title, reason); item.append(dot, copy)
    item.addEventListener('click', () => { state.telemetry.selection = { kind: value.kind, id: value.id }; renderObservability() }); return item
  }))
}
async function refreshObservabilityHistory(silent = true) {
  try {
    const value = await api('/api/v1/observability/history')
    state.telemetry.history.sessions = value.sessions || []
    state.telemetry.history.export = value.export || null
    state.telemetry.history.loaded = true
    renderObservationHistory()
  } catch (error) {
    if (!silent) toast(`Session history unavailable: ${error.message}`, true)
  }
}
async function selectObservationHistory(runId) {
  try {
    const value = await api(`/api/v1/observability/history/${encodeURIComponent(runId)}`)
    state.telemetry.history.selected = runId
    state.telemetry.history.samples = value.samples || []
    renderObservationHistory()
  } catch (error) { toast(`Historical telemetry unavailable: ${error.message}`, true) }
}
function renderObservationHistory() {
  const history = state.telemetry.history, container = $('#observe-history')
  if (!container) return
  const status = $('#observe-export-status'), exporter = history.export
  if (exporter?.configured) {
    status.className = exporter.last_error ? 'warning' : 'ready'
    status.textContent = exporter.last_error ? `Prometheus /metrics · OTLP retrying (${exporter.last_error})` : `Prometheus /metrics · OTLP ${exporter.last_success_unix_ms ? 'exporting' : 'configured'}`
    status.title = exporter.endpoint || ''
  } else if (exporter?.last_error) {
    status.className = 'warning'; status.textContent = `Prometheus /metrics · OTLP disabled (${exporter.last_error})`
  } else {
    status.className = ''; status.textContent = 'Prometheus /metrics · OTLP not configured'
  }
  if (!history.loaded) return replaceWithEmptyRow(container, 8, 'Loading persisted sessions…')
  if (!history.sessions.length) return replaceWithEmptyRow(container, 8, 'No persisted session yet. Start a Graph run to create one.')
  container.replaceChildren(...history.sessions.map((session) => {
    const row = document.createElement('tr'); row.className = session.health || 'healthy'
    if (history.selected === session.run_id) row.classList.add('selected')
    row.append(entityCell(`#${session.run_id.split('-').at(-1)}`, formatWallTime(session.started_at_unix_ms)), textCell(session.graph_id), textCell(formatDuration(session.duration_ms || 0)), textCell(formatNumber(session.frame_total)), textCell(formatNumber(session.max_queued)), textCell(formatNumber(session.drops_total)), textCell(`${Number(session.max_node_process_ms || 0).toFixed(2)}ms`), statusCell({ severity: session.health || 'healthy', reason: `${session.samples} persisted samples · ${session.status}` }))
    row.addEventListener('click', () => selectObservationHistory(session.run_id)); return row
  }))
  renderHistoryTrend()
}
function renderHistoryTrend() {
  const history = state.telemetry.history, container = $('#observe-history-trend')
  const samples = history.samples || []
  if (!history.selected || !samples.length) { container.classList.add('hidden'); container.replaceChildren(); return }
  container.classList.remove('hidden')
  const selected = history.sessions.find((session) => session.run_id === history.selected)
  const header = document.createElement('div'); header.className = 'history-trend-header'
  const copy = document.createElement('div'), title = document.createElement('b'), detail = document.createElement('small')
  title.textContent = `Session #${history.selected.split('-').at(-1)} trend`; detail.textContent = `${samples.length} persisted samples · ${selected?.status || 'unknown'} · ${formatDuration(selected?.duration_ms || 0)}`; copy.append(title, detail)
  const close = document.createElement('button'); close.className = 'mini-button'; close.textContent = 'Close trend'; close.addEventListener('click', () => { history.selected = null; history.samples = []; renderObservationHistory() })
  header.append(copy, close)
  const series = [
    ['Queued frames', samples.map((sample) => sumField(sample.edges, 'queue_len')), '', (value) => formatNumber(value)],
    ['Slowest Node avg', samples.map(maxNodeAverageMs), 'orange', (value) => `${value.toFixed(2)}ms`],
    ['Drops', samples.map((sample) => sumField(sample.edges, 'drop_total')), 'red', (value) => formatNumber(value)],
    ['Frames processed', samples.map((sample) => sumField(sample.edges, 'enqueue_total')), 'green', (value) => formatNumber(value)],
  ]
  const grid = document.createElement('div'); grid.className = 'history-trend-grid'; grid.append(...series.map(([name, values, color, formatter]) => sparkCard(name, values, color, formatter)))
  container.replaceChildren(header, grid)
}
function sparkCard(name, values, color, formatter) {
  const card = document.createElement('div'); card.className = `history-spark ${color}`
  const label = document.createElement('span'), current = document.createElement('b'), chart = svg('svg', { viewBox: '0 0 240 48', preserveAspectRatio: 'none', 'aria-label': `${name} trend` })
  label.textContent = name; current.textContent = formatter(values.at(-1) || 0)
  chart.append(svg('line', { x1: 0, y1: 47, x2: 240, y2: 47 }))
  const maximum = Math.max(1, ...values), divisor = Math.max(1, values.length - 1)
  const points = values.map((value, index) => `${index / divisor * 240},${46 - value / maximum * 43}`).join(' ')
  chart.append(svg('polyline', { points })); card.append(label, current, chart); return card
}
function sumField(values, field) { return (values || []).reduce((total, value) => total + Number(value[field] || 0), 0) }
function maxNodeAverageMs(sample) { return Math.max(0, ...(sample.nodes || []).map((node) => node.process_total ? Number(node.process_duration_ns || 0) / Number(node.process_total) / 1e6 : 0)) }
function formatWallTime(milliseconds) { if (!milliseconds) return 'Unknown start'; return new Date(milliseconds).toLocaleString([], { month: 'short', day: '2-digit', hour: '2-digit', minute: '2-digit', second: '2-digit' }) }
function renderObserveNodes(nodes) {
  if (!nodes.length) return replaceWithEmptyRow($('#observe-nodes'), 7, 'No Node telemetry. Run the Graph first.')
  $('#observe-nodes').replaceChildren(...nodes.map((node) => {
    const rate = state.telemetry.nodeRates.get(node.node_id)?.process || 0, verdict = nodeVerdict(node), graph = graphNode(node.node_id), average = node.process_total ? node.process_duration_ns / node.process_total : 0
    const row = document.createElement('tr'); row.className = verdict.severity; row.dataset.observeKind = 'node'; row.dataset.observeId = node.node_id
    if (state.telemetry.selection?.kind === 'node' && state.telemetry.selection.id === node.node_id) row.classList.add('selected')
    row.append(entityCell(node.node_id, graph?.node_type || 'runtime Node'), textCell(formatNumber(node.process_total)), textCell(`${formatRate(rate)}/s`), textCell(formatNanos(average)), textCell(formatNanos(node.max_process_duration_ns || 0)), textCell(formatCustomMetricSummary(node)), statusCell(verdict))
    row.addEventListener('click', () => { state.telemetry.selection = { kind: 'node', id: node.node_id }; renderObservability() }); return row
  }))
}
function renderObserveEdges(edges) {
  if (!edges.length) return replaceWithEmptyRow($('#observe-edges'), 8, 'No Edge telemetry. Run the Graph first.')
  $('#observe-edges').replaceChildren(...edges.map((edge) => {
    const rate = state.telemetry.edgeRates.get(edge.edge_id) || {}, verdict = edgeVerdict(edge), row = document.createElement('tr'); row.className = verdict.severity; row.dataset.observeKind = 'edge'; row.dataset.observeId = edge.edge_id
    if (state.telemetry.selection?.kind === 'edge' && state.telemetry.selection.id === edge.edge_id) row.classList.add('selected')
    row.append(entityCell(edge.edge_id, edgeRoute(edge.edge_id)), textCell(formatNumber(edge.enqueue_total)), textCell(`${formatRate(rate.frames || 0)}/s`), queueCell(edge), textCell(formatNanos(edge.oldest_frame_age_ns || 0)), textCell(formatNumber(edge.drop_total)), textCell(edge.audio_duration_ns_total ? `${(rate.mediaRatio || 0).toFixed(2)}×` : '—'), statusCell(verdict))
    row.addEventListener('click', () => { state.telemetry.selection = { kind: 'edge', id: edge.edge_id }; renderObservability() }); return row
  }))
}
function replaceWithEmptyRow(container, columns, message) { const row = document.createElement('tr'), cell = document.createElement('td'); cell.colSpan = columns; cell.className = 'observe-table-empty'; cell.textContent = message; row.append(cell); container.replaceChildren(row) }
function entityCell(titleValue, subtitleValue) { const cell = document.createElement('td'); cell.className = 'entity-cell'; const title = document.createElement('b'), subtitle = document.createElement('small'); title.textContent = titleValue; subtitle.textContent = subtitleValue; cell.append(title, subtitle); return cell }
function textCell(value) { const cell = document.createElement('td'); cell.textContent = value; return cell }
function statusCell(verdict) { const cell = document.createElement('td'), pill = document.createElement('span'); pill.className = `status-pill ${verdict.severity}`; pill.textContent = verdict.severity; pill.title = verdict.reason; cell.append(pill); return cell }
function queueCell(edge) { const cell = document.createElement('td'); cell.className = 'queue-cell'; const label = document.createElement('div'); label.className = 'queue-label'; label.textContent = `${edge.queue_len} / ${edge.queue_capacity}`; const meter = document.createElement('div'); meter.className = 'queue-meter'; const fill = document.createElement('i'); fill.style.width = `${Math.min(100, edge.queue_capacity ? edge.queue_len / edge.queue_capacity * 100 : 0)}%`; meter.append(fill); cell.append(label, meter); return cell }
function formatCustomMetricSummary(node) { const metrics = node.custom_metrics || []; if (!metrics.length) return '—'; return metrics.slice(0, 2).map((metric) => `${metric.name}=${formatNumber(metric.value)}`).join(' · ') + (metrics.length > 2 ? ` +${metrics.length - 2}` : '') }
function renderObserveDetail() {
  const selected = state.telemetry.selection, target = $('#observe-detail')
  if (!selected) { target.innerHTML = '<div class="observe-empty"><span>◎</span><b>Select a Node or Edge</b><p>Inspect the measurements behind its health verdict and get a concrete next action.</p></div>'; return }
  if (selected.kind === 'node') {
    const node = (state.runtime.nodes || []).find((value) => value.node_id === selected.id)
    if (!node) { state.telemetry.selection = null; return renderObserveDetail() }
    target.replaceChildren(buildNodeDetail(node)); return
  }
  const edge = (state.runtime.edges || []).find((value) => value.edge_id === selected.id)
  if (!edge) { state.telemetry.selection = null; return renderObserveDetail() }
  target.replaceChildren(buildEdgeDetail(edge))
}
function buildNodeDetail(node) {
  const root = document.createElement('div'), verdict = nodeVerdict(node), rate = state.telemetry.nodeRates.get(node.node_id) || {}, graph = graphNode(node.node_id)
  root.append(detailHeader('NODE', node.node_id, graph?.node_type || 'runtime Node', verdict), detailDiagnosis(verdict, nodeRecommendation(node, verdict)), detailMetrics([
    ['Process callbacks', formatNumber(node.process_total)], ['Current rate', `${formatRate(rate.process || 0)}/s`], ['Average process', formatNanos(node.process_total ? node.process_duration_ns / node.process_total : 0)], ['Maximum process', formatNanos(node.max_process_duration_ns || 0)], ['Signals', formatNumber(node.signal_total)], ['Errors / panics', `${node.error_total} / ${node.panic_total}`],
  ]))
  root.append(detailList('NODE-REPORTED METRICS', (node.custom_metrics || []).map((metric) => [metric.name, `${formatNumber(metric.value)} (${metric.kind})`]), 'This Node has not reported internal metrics.'))
  const connected = (state.runtime.edges || []).filter((edge) => { const item = graphEdge(edge.edge_id); return item && (item.from.node_id === node.node_id || item.to.node_id === node.node_id) }).map((edge) => [edge.edge_id, `${edge.queue_len}/${edge.queue_capacity} · ${edgeVerdict(edge).severity}`])
  root.append(detailList('CONNECTED EDGES', connected, 'No connected Edges.')); return root
}
function buildEdgeDetail(edge) {
  const root = document.createElement('div'), verdict = edgeVerdict(edge), rate = state.telemetry.edgeRates.get(edge.edge_id) || {}, graph = graphEdge(edge.edge_id)
  root.append(detailHeader('EDGE', edge.edge_id, edgeRoute(edge.edge_id), verdict), detailDiagnosis(verdict, edgeRecommendation(edge, verdict)), detailMetrics([
    ['Queue now', `${edge.queue_len} / ${edge.queue_capacity}`], ['Session high watermark', `${edge.high_watermark} / ${edge.queue_capacity}`], ['Frame rate', `${formatRate(rate.frames || 0)}/s`], ['Payload throughput', `${formatBytes(rate.bytes || 0)}/s`], ['Oldest frame', formatNanos(edge.oldest_frame_age_ns || 0)], ['Blocked total', formatNanos(edge.blocked_duration_ns || 0)], ['Dropped / full', `${edge.drop_total} / ${edge.full_total}`], ['Audio media speed', edge.audio_duration_ns_total ? `${(rate.mediaRatio || 0).toFixed(2)}× real time` : 'not an Audio Edge'],
  ]))
  root.append(detailList('ROUTING CONTRACT', [['From', graph ? `${graph.from.node_id}.${graph.from.port}` : 'unknown'], ['To', graph ? `${graph.to.node_id}.${graph.to.port}` : 'unknown'], ['Overflow', graph?.queue_policy?.overflow || 'block'], ['Latest error', edge.latest_error_reason || 'none']]))
  return root
}
function detailHeader(kind, titleValue, subtitle, verdict) { const section = document.createElement('div'); section.className = 'detail-header'; const eyebrow = document.createElement('span'); eyebrow.className = 'eyebrow'; eyebrow.textContent = kind; const title = document.createElement('h2'); title.textContent = titleValue; const copy = document.createElement('p'); copy.textContent = subtitle; const pill = statusCell(verdict).firstChild; pill.style.marginTop = '10px'; section.append(eyebrow, title, copy, pill); return section }
function detailDiagnosis(verdict, recommendation) { const section = document.createElement('section'); section.className = 'detail-section'; const heading = document.createElement('h3'); heading.textContent = 'DIAGNOSIS'; const copy = document.createElement('p'); copy.className = 'detail-diagnosis'; const strong = document.createElement('strong'); strong.textContent = `${verdict.reason}. `; copy.append(strong, document.createTextNode(recommendation)); const help = document.createElement('a'); help.className = 'observe-help'; help.href = 'https://piyotahu.github.io/muxiva/observability/'; help.target = '_blank'; help.rel = 'noreferrer'; help.textContent = 'Open troubleshooting guide ↗'; section.append(heading, copy, help); return section }
function detailMetrics(values) { const section = document.createElement('section'); section.className = 'detail-section'; const heading = document.createElement('h3'); heading.textContent = 'LIVE MEASUREMENTS'; const grid = document.createElement('div'); grid.className = 'detail-grid'; grid.append(...values.map(([labelValue, value]) => { const item = document.createElement('div'); item.className = 'detail-metric'; const label = document.createElement('span'), result = document.createElement('b'); label.textContent = labelValue; result.textContent = value; item.append(label, result); return item })); section.append(heading, grid); return section }
function detailList(titleValue, values, empty = 'No values.') { const section = document.createElement('section'); section.className = 'detail-section'; const heading = document.createElement('h3'); heading.textContent = titleValue; const list = document.createElement('div'); list.className = 'detail-list'; if (!values.length) { const item = document.createElement('div'); item.textContent = empty; list.append(item) } else list.append(...values.map(([name, value]) => { const item = document.createElement('div'), label = document.createElement('span'), code = document.createElement('code'); label.textContent = name; code.textContent = value; item.append(label, code); return item })); section.append(heading, list); return section }
function nodeRecommendation(node, verdict) { if (verdict.severity === 'healthy') return 'No action is required.'; const queueMs = Number(customMetric(node, 'ingress.queue_duration_ms') || 0); if (queueMs) return 'The Node owns an internal input queue. Increase its drain cadence or emit more than one buffered frame per tick; increasing a downstream Edge capacity only hides this delay.'; if (node.error_total || node.panic_total) return 'Open runtime.log and search this Node ID to inspect its latest callback failure.'; return 'Inspect connected Edges first. If their queues are healthy, profile this Node callback or move blocking I/O off the callback path.' }
function edgeRecommendation(edge, verdict) { if (verdict.severity === 'healthy') return 'No action is required.'; if (edge.drop_total) return 'Frames have already been lost. Fix the slow consumer or the producer rate before changing overflow policy.'; if (edge.queue_len) return 'Compare producer and consumer rates, then inspect the destination Node. A larger capacity delays failure but increases end-to-end latency.'; return 'Inspect the destination Node callback and runtime.log for blocking or errors.' }
function formatRate(value) { if (!Number.isFinite(value)) return '0'; return value >= 100 ? Math.round(value).toString() : value >= 10 ? value.toFixed(1) : value.toFixed(2) }
function formatNumber(value) { return new Intl.NumberFormat('en-US', { notation: Number(value) >= 100000 ? 'compact' : 'standard', maximumFractionDigits: 1 }).format(Number(value) || 0) }
function formatBytes(value) { if (value < 1024) return `${formatRate(value)} B`; if (value < 1048576) return `${formatRate(value / 1024)} KiB`; return `${formatRate(value / 1048576)} MiB` }
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
  if (!await validateGraph(false)) { toast('Fix validation errors before saving', true); return false }
  try {
    await api('/api/v1/graph', { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(state.graph) })
    setDirty(false); toast('Graph saved to disk'); return true
  } catch (error) { toast(error.message, true); return false }
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

function setZoom(value, pointer = null) {
  const next = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, value))
  if (Math.abs(next - state.zoom) < .0001) return
  state.zoom = next
  applyViewport()
  if (pointer) {
    const shiftedAnchor = canvasPoint(pointer.clientX, pointer.clientY)
    state.viewport.x += pointer.anchor.x - shiftedAnchor.x
    state.viewport.y += pointer.anchor.y - shiftedAnchor.y
    applyViewport()
  }
}
function fitView() {
  if (!state.graph.nodes.length) {
    state.viewport = { x: VIEWPORT_WIDTH / 2, y: VIEWPORT_HEIGHT / 2 }
    state.zoom = 1
    applyViewport()
    return
  }
  const bounds = state.graph.nodes.reduce((result, node) => {
    const position = state.positions[node.id]
    if (!position) return result
    result.left = Math.min(result.left, position.x)
    result.top = Math.min(result.top, position.y)
    result.right = Math.max(result.right, position.x + 220)
    result.bottom = Math.max(result.bottom, position.y + nodeHeight(nodeInfo(node)))
    return result
  }, { left: Infinity, top: Infinity, right: -Infinity, bottom: -Infinity })
  const padding = 90
  const contentWidth = Math.max(1, bounds.right - bounds.left + padding * 2)
  const contentHeight = Math.max(1, bounds.bottom - bounds.top + padding * 2)
  state.viewport = { x: (bounds.left + bounds.right) / 2, y: (bounds.top + bounds.bottom) / 2 }
  state.zoom = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, Math.min(VIEWPORT_WIDTH / contentWidth, VIEWPORT_HEIGHT / contentHeight)))
  applyViewport()
}
function toast(message, error = false) {
  const element = $('#toast'); element.textContent = message; element.classList.toggle('error', error); element.classList.add('show')
  clearTimeout(toast.timer); toast.timer = setTimeout(() => element.classList.remove('show'), 2400)
}
function fatal(message) { $('#fatal-message').textContent = message; $('#fatal').classList.remove('hidden') }

loadStudio()
