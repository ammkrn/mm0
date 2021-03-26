export const LOADING: string = "loading...";
export const STACKLOADING: string = `<li class="stack_item"> ${LOADING} </li>`;
export const HEAPLOADING: string = `<li class="heap_item"> ${LOADING} </li>`;

export class State {
    num: number;
    mode: string;
    stack: string;
    heap: string;
    ustack: string;
    uheap: string;
    hstack: string;
    cmd: string;
    constructor(
        num: number,
        mode: string,
        stack: string,
        heap: string,
        ustack: string,
        uheap: string,
        hstack: string,
        cmd: string,
    ) {
        this.num = num;
        this.mode = mode;
        this.stack = stack;
        this.heap = heap;
        this.ustack = ustack;
        this.uheap = uheap;
        this.hstack  = hstack;
        this.cmd = cmd;
    }    
}

export class SubUnify {
    subnum: number;
    subof: number;
    tgt: string;
    finish: string;

    constructor(subnum: number, subof: number, tgt: string, finish: string) {
        this.subnum = subnum;
        this.subof = subof;
        this.tgt = tgt;
        this.finish = finish;
    }
}

export class ProofState extends State {
    subunify : SubUnify | undefined;
    constructor(
        num: number,
        mode: string,
        stack: string,
        heap: string,
        ustack: string,
        uheap: string,
        hstack: string,
        cmd: string,
        subunify: SubUnify | undefined,
    ) {
        super(num, mode, stack, heap, ustack, uheap, hstack, cmd);
        this.subunify = subunify;
    }
}

export function try_proof_state(input: unknown): ProofState | undefined {
    let x = input as ProofState;
    if (x.num === undefined) {
        console.log('num undefined')
        console.log(`input: ${JSON.stringify(x)}`);
    } else if (x.mode === undefined) {
        console.log('mode undefined');
        console.log(`input: ${JSON.stringify(x)}`);

    } else if (x.stack === undefined) {
        console.log('stack undefined');
        console.log(`input: ${JSON.stringify(x)}`);
        
    } else if (x.heap === undefined) {
        console.log('heap undefined');
        console.log(`input: ${JSON.stringify(x)}`);
        
    } else if (x.ustack === undefined) {
        console.log('ustack undefined');
        console.log(`input: ${JSON.stringify(x)}`);
        
    } else if (x.uheap === undefined) {
        console.log('uheap undefined');
        console.log(`input: ${JSON.stringify(x)}`);
        
    } else if (x.hstack === undefined) {
        console.log('stack undefined');
        console.log(`input: ${JSON.stringify(x)}`);

    } else if (x.cmd === undefined) {
        console.log('cmd undefined');
        console.log(`input: ${JSON.stringify(x)}`);
    }
    if (
        (x.num === undefined)
        || (x.mode === undefined)
        || (x.stack === undefined)
        || (x.heap === undefined)
        || (x.ustack === undefined)
        || (x.uheap === undefined)
        || (x.hstack === undefined)
        || (x.cmd === undefined)
    ) {
        return undefined;
    } else {
        return x;
    }
} 


export const PROOFLOADING = new ProofState(
    0, 
    LOADING,
    STACKLOADING,
    HEAPLOADING,
    STACKLOADING,
    HEAPLOADING,
    STACKLOADING,
    LOADING,
    undefined
);

export class UnifyState extends State {
    tgt: string;

    constructor(
        num: number,
        mode: string,
        stack: string,
        heap: string,
        ustack: string,
        uheap: string,
        hstack: string,
        cmd: string,
        tgt: string, 
    ) {
        super(num, mode, stack, heap, ustack, uheap, hstack, cmd);
        this.tgt = tgt;
    }    



}

export function try_unify_state(input: unknown): UnifyState | undefined {
    let x = input as UnifyState;
    if (x.num === undefined) {
        console.log('unify undefined num');
        console.log(`input: ${JSON.stringify(x)}`);
    } else if (x.mode === undefined) {
        console.log('unify undefined mode');
        console.log(`input: ${JSON.stringify(x)}`);
    } else if (x.stack === undefined) {
        console.log('unify undefined stack');
        console.log(`input: ${JSON.stringify(x)}`);
    } else if (x.heap === undefined) {
        console.log('unify undefined heap');
        console.log(`input: ${JSON.stringify(x)}`);
    } else if (x.ustack === undefined) {
        console.log('unify undefined ustack');
        console.log(`input: ${JSON.stringify(x)}`);
    } else if (x.hstack === undefined) {
        console.log('unify undefined hstack');
        console.log(`input: ${JSON.stringify(x)}`);
    } else if (x.uheap === undefined) {
        console.log('unify undefined uheap');
        console.log(`input: ${JSON.stringify(x)}`);
    } else if (x.cmd === undefined) {
        console.log('unify undefined cmd');
        console.log(`input: ${JSON.stringify(x)}`);
    } else if (x.tgt === undefined) {
        console.log('unify undefined tgt');
        console.log(`input: ${JSON.stringify(x)}`);
    } else {
        console.log('fields OK')
    }

    if (
        (x.num === undefined)
        || (x.mode === undefined)
        || (x.stack === undefined)
        || (x.heap === undefined)
        || (x.ustack === undefined)
        || (x.uheap === undefined)
        || (x.hstack === undefined)
        || (x.cmd === undefined)
        || (x.tgt === undefined)
    ) {
        return undefined;
    } else {
        return x;
    }
}

export const UNIFYLOADING = new UnifyState(
    0, 
    LOADING,
    STACKLOADING,
    HEAPLOADING,
    STACKLOADING,
    HEAPLOADING,
    STACKLOADING,
    LOADING,
    LOADING,
);



