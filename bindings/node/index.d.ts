/// <reference types="node" />

export type FrameKind = 'audio'|'video'|'text'|'byte'|'signal'|'event'
export class Frame { readonly kind: FrameKind; readonly sequence: number; copy(): Frame }
export class TextFrame { constructor(text:string, sequence:number); readonly text:string; readonly kind:'text'; readonly sequence:number; asFrame():Frame }
export class ByteFrame { constructor(bytes:Buffer, mediaType:string|undefined, sequence:number); readonly bytes:Buffer; readonly mediaType?:string; readonly kind:'byte'; readonly sequence:number; asFrame():Frame }
export class AudioFrame { constructor(bytes:Buffer, sampleRateHz:number, channels:number, format:'u8'|'i16le'|'i24le'|'i32le'|'f32le'|'f64le', planar:boolean, samplesPerChannel:number, sequence:number); readonly bytes:Buffer; readonly kind:'audio'; readonly sequence:number; asFrame():Frame }
export class VideoFrame { constructor(bytes:Buffer,width:number,height:number,stride:number,sequence:number); readonly bytes:Buffer; readonly width:number; readonly height:number; readonly kind:'video'; readonly sequence:number; asFrame():Frame }
export class SignalFrame { constructor(name:string,source:string,schemaVersion:number,payloadJson:string,sequence:number); readonly name:string; readonly payloadJson:string; readonly kind:'signal'; asFrame():Frame }
export class EventFrame { constructor(topic:string,source:string,schemaVersion:number,payloadJson:string,sequence:number); readonly topic:string; readonly payloadJson:string; readonly kind:'event'; asFrame():Frame }
export class Runtime { constructor(); readonly isClosed:boolean; createSession():Session; close():boolean }
export class Session { readonly id:number; readonly isClosed:boolean; close():boolean }
export class EventBus { constructor(); subscribe(topic:string,callback:(payload:string)=>void,capacity?:number):number; publish(topic:string,payload:string):number; unsubscribe(id:number):boolean; close():boolean }
export interface DomainCommand { sequence:number; kind:string; payloadJson?:string }
export class NodeExecutionDomain { constructor(callback:(command:DomainCommand)=>void,capacity:number); submit(sequence:number,kind:string,payloadJson?:string):'accepted'|'full'|'closed'; complete(sequence:number,value:string):boolean; fail(sequence:number,code:string,message:string,value:string):boolean; drainCompletions():string[]; readonly outstanding:number; close():boolean }

/** A JSON-serializable or structured-clone-compatible value crossing the Worker boundary. */
export type NodeValue = null | boolean | number | string | NodeValue[] | { [key:string]: NodeValue }
export interface NodeOptions { capacity?:number }
export interface TransformImplementation<Input = NodeValue, Output = Input, Signal = NodeValue, Event = NodeValue> {
  onPrepare?():void
  onProcess(frame:Input):Output | Output[] | null | undefined
  onSignal?(frame:Signal):void
  onEvent?(frame:Event):void
  onFinish?():void
  onAbort?(reason:unknown):void
}
export function defineTransformNode<Input = NodeValue, Output = Input, Signal = NodeValue, Event = NodeValue>(implementation:TransformImplementation<Input,Output,Signal,Event>):TransformImplementation<Input,Output,Signal,Event>
export class TypeScriptTransformNode<Input = NodeValue, Output = Input, Signal = NodeValue, Event = NodeValue> {
  constructor(implementation:TransformImplementation<Input,Output,Signal,Event>,options?:NodeOptions)
  prepare():Promise<unknown>
  process(frame:Input):Promise<Output | Output[] | null | undefined>
  signal(frame:Signal):Promise<unknown>
  event(frame:Event):Promise<unknown>
  finish():Promise<unknown>
  abort(reason:unknown):Promise<unknown>
  close():Promise<boolean>
  readonly outstanding:number
}
export class NodeRunner<Input = NodeValue, Output = Input, Signal = NodeValue, Event = NodeValue> {
  constructor(implementation:TransformImplementation<Input,Output,Signal,Event>,options?:NodeOptions)
  readonly domain:TypeScriptTransformNode<Input,Output,Signal,Event>
  readonly outstanding:number
  start():Promise<this>
  process(frame:Input):Promise<Output | Output[] | null | undefined>
  signal(frame:Signal):Promise<unknown>
  event(frame:Event):Promise<unknown>
  finish():Promise<boolean>
  abort(reason:unknown):Promise<unknown>
  close():Promise<boolean>
}

export interface GraphNodeFactoryOptions {
  version?:string
  inputPort?:string
  outputPort?:string
  kind?:'source'|'transform'|'sink'
  ports?:GraphPortDescriptor[]
  configSchema?:unknown
}
export type GraphFrameType = 'audio'|'video'|'text'|'byte'
export interface GraphPortDescriptor { name:string; direction:'input'|'output'; frameType:GraphFrameType }
export interface GraphNodeContext { nodeId:string; inputPort?:string; config:Record<string,unknown> }
export type GraphTextFrame = { kind:'text'; sequence:number; text:string }
export type GraphByteFrame = { kind:'byte'; sequence:number; bytes:number[]; mediaType?:string }
export type GraphAudioFrame = { kind:'audio'; sequence:number; bytes:number[]; sampleRateHz:number; channels:number; format:'u8'|'i16le'|'i24le'|'i32le'|'f32le'|'f64le'; planar:boolean; samplesPerChannel:number }
export type GraphVideoFrame = { kind:'video'; sequence:number; pixelFormat:'rgba8'; bytes:number[]; width:number; height:number; stride:number }
export type GraphFrame = GraphTextFrame|GraphByteFrame|GraphAudioFrame|GraphVideoFrame
export type GraphEmissions = GraphFrame|Record<string,GraphFrame|GraphFrame[]>|null|undefined
export interface GraphNodeImplementation<Input = GraphFrame, Output = GraphEmissions> {
  onPrepare?(frame:undefined, context:GraphNodeContext):unknown
  onProcess(frame:Input, context:GraphNodeContext):Output
  onSignal?(frame:unknown, context:GraphNodeContext):unknown
  onFinish?(frame:undefined, context:GraphNodeContext):unknown
  onAbort?(reason:unknown, context:GraphNodeContext):unknown
}
export class GraphNodeFactory<Input = GraphFrame, Output = GraphEmissions> {
  constructor(nodeType:string,implementation:GraphNodeImplementation<Input,Output>,options?:GraphNodeFactoryOptions)
  readonly spec:{ nodeType:string; version:string; inputPort:string; outputPort:string }
}
export function runGraph(graphJson:string,factories:GraphNodeFactory[],options?:{ timeoutMs?:number }):Promise<number>
