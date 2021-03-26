import { DebuggerMeta } from './infoview';
import { 
    State, 
    ProofState, 
    UnifyState, 
    PROOFLOADING, 
    UNIFYLOADING, 
} from './containers';

export enum ActiveStream {
    Proof,
    Unify
}

export interface DebugStream<A> {
    cur_state: A;
    cached_states: Map<number, A>;
    cachedTable: string | undefined;
    identify: ActiveStream;
    htmlBody(meta: DebuggerMeta): string;
}

export class ProofStream implements DebugStream<ProofState> {
    cur_state: ProofState;
    cached_states: Map<number, ProofState>;
    cachedTable: string | undefined;
    identify: ActiveStream = ActiveStream.Proof;

    constructor() {
        this.cur_state = PROOFLOADING;
        this.cached_states = new Map<number, ProofState>();
    }

    htmlBody(meta: DebuggerMeta): string {
        let progress = `${this.cur_state.num}/${meta.total_proof_steps}`
        // The info displayed is going to depend on whether 
        let [tgt, finish, uprogress] = (this.cur_state.subunify === undefined) 
        ? ["", "", ""]
        : [
            `${this.cur_state.subunify.tgt}`,
            `${this.cur_state.subunify.finish}`,
            `(sub-unify: ${this.cur_state.subunify.subnum}/${this.cur_state.subunify.subof})`
        ];

        return `<div class="outer_flex">
                    <div class="verif_grid use_editor_font">
                        <ul class="stack"> ${this.cur_state.stack} </ul>
                        <ul class="stack"> ${this.cur_state.ustack} </ul>
                        <ul class="stack"> ${this.cur_state.hstack} </ul>
                        <ul class="heap"> ${this.cur_state.heap} </ul>
                        <ul class="heap"> ${this.cur_state.uheap} </ul>
                        <ul class="heap"> ${meta.vars} </ul>
                    </div> 

                    <div class="flex-container use_editor_font">
                        <div> Proof: ${meta.styled_decl_ident} </div>
                        <div> Mode : ${this.cur_state.mode} </div>
                        <div> Unify target: ${tgt} </div>
                        <div> Resume: ${finish} </div>
                        <div> Step: ${progress} ${uprogress} </div>
                        <div> Next: ${this.cur_state.cmd} </div>
                        <div>
                            <form id="skip_to_form">
                                <label for="jumpto">Jump to: </label>
                                <input type="number" id="skip_to_val" min="0" max="${meta.total_proof_steps}">
                            </form>
                        </div>
                    </div>
                </div>`;
    }
}

export class UnifyStream implements DebugStream<UnifyState> {
    cur_state: UnifyState;
    cached_states: Map<number, UnifyState>;
    cachedTable: string | undefined;
    identify: ActiveStream = ActiveStream.Unify;

    constructor() {
        this.cur_state = UNIFYLOADING;
        this.cached_states = new Map<number, UnifyState>();
    }

    htmlBody(meta: DebuggerMeta): string {
        let progress = `${this.cur_state.num}/${meta.total_unify_steps}`
        return `<div class="outer_flex">
                    <div class="verif_grid use_editor_font">
                        <ul class="stack"> ${this.cur_state.stack} </ul>
                        <ul class="stack"> ${this.cur_state.ustack} </ul>
                        <ul class="stack"> ${this.cur_state.hstack} </ul>
                        <ul class="heap"> ${this.cur_state.heap} </ul>
                        <ul class="heap"> ${this.cur_state.uheap} </ul>
                        <ul class="heap"> ${meta.vars} </ul>
                    </div> 
                    <div class="flex-container use_editor_font">
                        <div> Unify: ${meta.styled_decl_ident} </div>
                        <div> Mode : ${this.cur_state.mode} </div>
                        <div> Unify target: ${this.cur_state.tgt} </div>
                        <div> Step: ${progress} </div>
                        <div> Next: ${this.cur_state.cmd} </div>
                        <div>
                            <form id="skip_to_form">
                                <label for="jumpto">Jump to: </label>
                                <input type="number" id="skip_to_val" min="0" max="${meta.total_unify_steps}">
                            </form>
                        </div>
                    </div>
                </div>`;
    }
}

export function tableBody<A extends State>(meta: DebuggerMeta, stream: DebugStream<A>, rows: string): string {
    let total_num_steps = (stream.identify === ActiveStream.Unify) ? meta.total_unify_steps : meta.total_proof_steps;
    let progress = `${stream.cur_state.num}/${total_num_steps}`
    return `<div class="cmd_table use_editor_font">
                <table>
                    <tr>
                        <th>num</th>
                        <th>cmd</th>
                        <th>mode</th>
                        <th>data</th>
                    </tr>
                    ${rows}
                </table>
            </div> 
            <div class="flex-container-sticky use_editor_font">
                <div>
                    <div> declaration: ${meta.styled_decl_ident} </div>
                    <!-- <div id="table_progress"> Step: ${progress} </div> --!>
                    <form id="skip_to_form">
                        <label for="jumpto">Jump to: </label>
                        <input type="number" id="skip_to_val" min="0" max="${total_num_steps}">
                    </form>
                </div>
            </div>`;        
}
