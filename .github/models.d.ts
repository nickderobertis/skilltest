export type Result = "success" | "failure" | "cancelled" | "skipped";
export interface Outcome { result: Result; outputs: Record<string,string>; }
export interface Step { id?: string; run?: string; uses?: string; with?: Record<string,unknown>; if?: string|boolean; env?: Record<string,unknown>; }
export interface Job { name?: string; if?: string|boolean; needs?: string|string[]; uses?: string; "runs-on"?: string|string[]; strategy?: {matrix?:Record<string,unknown>}; steps?: Step[]; with?:Record<string,unknown>; outputs?:Record<string,string>; }
export interface Input {default?:unknown;}
export interface Trigger {branches?:string[];tags?:string[];types?:string[];paths?:string[];workflows?:string[];inputs?:Record<string,Input>;}
export interface Workflow {on:Record<string,Trigger|null>;jobs:Record<string,Job>;}
export type Graph = Record<string,Workflow>;
export interface Scenario {event_name:string; ref:string; label?:string;action?:string;changed:string[];inputs?:Record<string,unknown>;sender?:{login?:string};repository?:{full_name:string};head_commit?:{message:string};pull_request?:{title?:string;head?:{ref?:string;repo?:{full_name?:string}};base?:{ref?:string}};workflow_run?:{name?:string;head_branch?:string;head_sha?:string;head_repository?:{full_name?:string};conclusion?:string;event?:string;id?:number};}
export interface Classification {isReleaseOnly?:boolean;macosRequired:boolean;reason?:string;}
export interface Row {workflow:string;event:string;job:string;name:string;runner:string;target:unknown;requested:boolean;result:Result;caller?:string;}
export interface ResolvedRun {rows:Row[];results:Record<string,Outcome>;}
export interface RecordedRun {id:string;scenario:string;event:string;workflow:string;manual:boolean;declaredJobs:string[];jobs:Row[];}
export interface Lifecycle {id:string;runs:string[];optionalPublish:Record<string,boolean>;}
export interface ScenarioTrigger {event_name:string;ref:string;action?:string;changed:string[];base_ref?:string;workflow_run_name?:string;workflow_run_head_branch?:string;}
export interface RecordedScenario {label:string;trigger:ScenarioTrigger;workflows:string[];classification:{isReleaseOnly:boolean;macosRequired:boolean;reason:string};}
export interface Telemetry {schema_version:2;sourceDigest:string;scenarios:RecordedScenario[];runs:RecordedRun[];lifecycles:Lifecycle[];}
export interface Observation {head:string;runs:{name:string;html_url:string;status:string;conclusion:string|null;jobs:{name:string;status:string;conclusion:string|null}[]}[];}
