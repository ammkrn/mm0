import { DebuggerMeta } from "./infoview";
import { State, ProofState, UnifyState } from './containers';

export interface ServerResponse<A extends State> {
    meta: DebuggerMeta;
    states: A[];
    table: string | undefined;
    error: string | undefined;
}

export class ProofResponse implements ServerResponse<ProofState> {
    meta: DebuggerMeta;
    states: ProofState[];
    table: string | undefined;
    error: string | undefined;

    constructor(meta: DebuggerMeta, data: ProofState[], error: string | undefined) {
        this.meta = meta;
        this.states = data;
        this.error = error;
    }
}

export class UnifyResponse implements ServerResponse<UnifyState> {
    meta: DebuggerMeta;
    states: UnifyState[];
    table: string | undefined;
    error: string | undefined;

    constructor(meta: DebuggerMeta, data: UnifyState[], error: string | undefined) {
        this.meta = meta;
        this.states = data;
        this.error = error;
    }
}
