export type FrameKind = 'audio'|'video'|'text'|'byte'|'signal'|'event'
export class Frame { readonly kind: FrameKind; readonly sequence: number; copy(): Frame }
export class TextFrame { constructor(text:string, sequence:number); readonly text:string; readonly kind:'text'; readonly sequence:number; asFrame():Frame }
export class ByteFrame { constructor(bytes:Buffer, mediaType:string|undefined, sequence:number); readonly bytes:Buffer; readonly mediaType?:string; readonly kind:'byte'; readonly sequence:number; asFrame():Frame }
export class AudioFrame { constructor(bytes:Buffer, sampleRateHz:number, channels:number, format:'u8'|'i16le'|'i24le'|'i32le'|'f32le'|'f64le', planar:boolean, samplesPerChannel:number, sequence:number); readonly bytes:Buffer; readonly kind:'audio'; readonly sequence:number; asFrame():Frame }
export class VideoFrame { constructor(bytes:Buffer,width:number,height:number,stride:number,sequence:number); readonly bytes:Buffer; readonly width:number; readonly height:number; readonly kind:'video'; asFrame():Frame }
export class SignalFrame { constructor(name:string,source:string,schemaVersion:number,payloadJson:string,sequence:number); readonly name:string; readonly payloadJson:string; readonly kind:'signal'; asFrame():Frame }
export class EventFrame { constructor(topic:string,source:string,schemaVersion:number,payloadJson:string,sequence:number); readonly topic:string; readonly payloadJson:string; readonly kind:'event'; asFrame():Frame }
export class Runtime { constructor(); readonly isClosed:boolean; createSession():Session; close():boolean }
export class Session { readonly id:number; readonly isClosed:boolean; close():boolean }
export class EventBus { constructor(); subscribe(topic:string,callback:(payload:string)=>void,capacity?:number):number; publish(topic:string,payload:string):number; unsubscribe(id:number):boolean; close():boolean }
export interface DomainCommand { sequence:number; kind:string; payloadJson?:string }
export class NodeExecutionDomain { constructor(callback:(command:DomainCommand)=>void,capacity:number); submit(sequence:number,kind:string,payloadJson?:string):'accepted'|'full'|'closed'; complete(sequence:number,value:string):boolean; fail(sequence:number,code:string,message:string,value:string):boolean; drainCompletions():string[]; readonly outstanding:number; close():boolean }
export interface TransformImplementation { onPrepare?():unknown; onProcess?(frame:unknown):unknown; onSignal?(frame:unknown):unknown; onFinish?():unknown; onAbort?(reason:unknown):unknown }
export class TypeScriptTransformNode { constructor(implementation:TransformImplementation,options?:{capacity?:number}); prepare():Promise<unknown>; process(frame:unknown):Promise<unknown>; signal(frame:unknown):Promise<unknown>; finish():Promise<unknown>; abort(reason:unknown):Promise<unknown>; close():Promise<boolean>; readonly outstanding:number }

