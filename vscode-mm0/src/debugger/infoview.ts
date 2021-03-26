import { mm0Client } from '../extension';
import * as vc from 'vscode';
import { LanguageClient } from 'vscode-languageclient';
import * as themes from './themes';
import { State, ProofState, UnifyState, PROOFLOADING, UNIFYLOADING, LOADING, HEAPLOADING } from './containers';
import { ServerResponse } from './response';
import { DebugStream, ProofStream, UnifyStream, ActiveStream, tableBody } from './streams';

export async function infoviewDropdown(): Promise<string | undefined> {
    const options: vc.InputBoxOptions = {
    	placeHolder: "<name of declaration to debug>",
		ignoreFocusOut: false,
    	prompt: "Enter the name of the declaration (term, axiom, theorem, or definition)' ",
		// We don't validate in real-time since it's not cheap to do without storing state between queries.
    };
	
	// The string the user entered.
	const request = await vc.window.showInputBox(options);

	// Return early iff the user pressed esscape or unfocused
	if (!request) return undefined;
	return request;
}

function showError(e: any) {
    vc.window.showErrorMessage('error: ' + e);
    console.log('error: ' + e);
}

export enum Wview {
    State = "State",
    Table = "Table"
}

export class DebuggerMeta {
    decl_kind: string;
    decl_num: number;
    decl_ident: string;
    styled_decl_ident: string;
    total_proof_steps: number;
    total_unify_steps: number;
    vars: string;

    constructor(
        decl_kind: string,
        decl_num: number,
        decl_ident: string,
        styled_decl_ident: string,
        total_proof_steps: number,
        total_unify_steps: number,
        vars: string,
    ) {
        this.decl_kind = decl_kind;
        this.decl_num = decl_num;
        this.decl_ident = decl_ident;
        this.styled_decl_ident = styled_decl_ident;
        this.total_proof_steps = total_proof_steps;
        this.total_unify_steps = total_unify_steps;
        this.vars = vars;
    }
}

const InitialMeta = new DebuggerMeta(
    LOADING,
    0, 
    LOADING,
    LOADING,
    0, 
    0, 
    HEAPLOADING,
);

export class MmbDebugView {
    private stateView: vc.WebviewPanel | undefined
    private tableView: vc.WebviewPanel | undefined
    private meta: DebuggerMeta;
    private elabLevel: number;
    private bracketLevel: number;
    private proofStream: ProofStream;
    private unifyStream: UnifyStream;
    private context: vc.ExtensionContext;
    private active: ActiveStream;
    // The lsp Uri type is a string. Don't keep converting it.
    private fileUri: string;
    private mmFileWatcher: vc.FileSystemWatcher

    constructor(context: vc.ExtensionContext, decl_ident: string, fileUri: string) {
        this.meta = InitialMeta;
        this.meta.decl_ident = decl_ident;
        // default elab level is max.
        this.elabLevel = 2;
        this.bracketLevel = 0;
        this.fileUri = fileUri;
        this.proofStream = new ProofStream();
        this.unifyStream = new UnifyStream();
        this.active = ActiveStream.Proof;
        this.context = context;

        // Reload the debugger with new contents in the event of any metamath file changes.
        this.mmFileWatcher = vc.workspace.createFileSystemWatcher('**/*.mm*', false, false, false);
        this.mmFileWatcher.onDidChange(() => {
            this.reload()
        })
    }

    activeStream(): DebugStream<ProofState | UnifyState> {
        switch (this.active) {
            case ActiveStream.Proof: 
                return this.proofStream
            case ActiveStream.Unify: 
                return this.unifyStream
        }
    }

    getView(wview: Wview): vc.WebviewPanel | undefined {
        switch (wview) {
            case Wview.State: 
                return this.stateView
            case Wview.Table: 
                return this.tableView
        }
    }

    // the `r` key in the debugger will reload debugger after flushing
    // the cached steps so that any changes in the mmb file will be reflected.
    // This currently resets both step counters to zero, since the newly fetched
    // proof file may have a different number of steps. If, for instance, the debugger is on
    // proof step 400, but the new proof only has 100 steps, the extension is going to ask for a chunk
    // of steps that don't exist and get an empty response.
    // It's possible to avoid this behavior by having a 'get meta only' preliminary request,
    // to know how many steps will be in the 
    private async reload() {
        this.proofStream.cached_states = new Map<number, ProofState>();
        this.proofStream.cachedTable = undefined;
        this.unifyStream.cached_states = new Map<number, UnifyState>();
        this.unifyStream.cachedTable = undefined;
        this.proofStream.cur_state = PROOFLOADING;
        this.unifyStream.cur_state = UNIFYLOADING;
        // The argument step number will always be 0 at this point.
        return this.trySetState(this.activeStream().cur_state.num, true);
    }

    private async clearCachedSteps() {
        this.proofStream.cached_states = new Map<number, ProofState>();
        this.unifyStream.cached_states = new Map<number, UnifyState>();
        this.proofStream.cur_state = PROOFLOADING;
        this.unifyStream.cur_state = UNIFYLOADING;
        return;
    }

    async openViewIfClosed(wview: Wview, snum: number): Promise<void> {
        switch (wview) {
            case Wview.State: {
                if (this.stateView !== undefined) {
                    this.stateView.reveal();
                    return;
                } else {
                    break;
                }
            }
            case Wview.Table: {
                if (this.tableView !== undefined) {
                    this.tableView.reveal();
                    return;
                } else {
                    break;
                }
            }
            default: break;
        }

        await this.serverReq(this.activeStream(), snum).catch((err) => { throw err });
        // Create the webview
        let this_view = vc.window.createWebviewPanel(
            'metamath-zero mmb debugger', 
            `debug ${this.meta.decl_ident} ${wview}`,
            {
                // `Active` means it will open it in a new tab.
                viewColumn: vc.ViewColumn.Active,
                preserveFocus: false
            },
            {
                enableFindWidget: true,
                retainContextWhenHidden: true,
                enableCommandUris: true,
                enableScripts: true
            }
        );

        // This handles messages sent from the scripts embedded in the webview,
        // which is how we get navigation keystrokes and skip_to commands.
        this_view.webview.onDidReceiveMessage(
            async message => {
                switch (message.command) {
                    case 'step_fwd': {
                        await this.trySetState(this.activeStream().cur_state.num + 1, false).catch((e) => showError(e));
                        break;
                    }
                    case 'step_back': {
                        await this.trySetState(this.activeStream().cur_state.num - 1, false).catch((e) => showError(e));
                        break;
                    }
                    case 'reload': {
                        this.reload().catch((e) => showError(e));
                        // Even though there are other situations in which we reload (file changes), 
                        // only show the pop-up if they explicitly request the reload so the user 
                        // positive feedback that they got what they asked for.
                        vc.window.showInformationMessage(`reloaded: ${this.fileUri}`);
                        break;
                    }
                    case 'viewProof': {
                        if (this.active !== ActiveStream.Proof) {
                            await this.trySetState(this.proofStream.cur_state.num, true, this.proofStream).catch((e) => showError(e));
                        }
                        break;
                    }
                    case 'viewUnify': {
                        if (this.active !== ActiveStream.Unify) {
                            await this.trySetState(this.unifyStream.cur_state.num, true, this.unifyStream).catch((e) => showError(e));
                        }
                        break;
                    }
                    case 'viewState': {
                        await this.openViewIfClosed(Wview.State, this.activeStream().cur_state.num).catch((e) => showError(e));
                        break;
                    }
                    case 'viewTable': {
                        await this.openViewIfClosed(Wview.Table, this.activeStream().cur_state.num).catch((e) => showError(e));
                        break;
                    }
                    case 'elabLevel0': {
                        await this.changeElabLevel(this.activeStream().cur_state.num, false, 0).catch((e) => showError(e));
                        break;
                    }
                    case 'elabLevel1': {
                        await this.changeElabLevel(this.activeStream().cur_state.num, false, 1).catch((e) => showError(e));
                        break;
                    }
                    case 'elabLevel2': {
                        await this.changeElabLevel(this.activeStream().cur_state.num, false, 2).catch((e) => showError(e));
                        break;
                    }
                    case 'bracketLevel': {
                        await this.changeBracketLevel(this.activeStream().cur_state.num, false).catch((e) => showError(e));
                        break;
                    }
                    case 'skip_to': {
                        await this.trySetState(parseInt(message.text, 10), false).catch((e) => showError(e));
                        break;
                    }
                    // Some unmapped key; do nothing.
                    default: break;
                }
            },
            // onDidReceiveMessage thisArgs
            undefined,
            // onDidReceiveMessage disposables
            this.context.subscriptions
        );

        switch (wview) {
            case Wview.State: {
                this_view.onDidDispose(() => {
                    this.stateView = undefined;
                })
                this.stateView = this_view;
                break;
            }
            case Wview.Table: {
                this_view.onDidDispose(() => {
                    this.tableView = undefined;
                })
                this.tableView = this_view;
                break;
            }
        }

        await this.trySetState(this.activeStream().cur_state.num, true);
    }



// return is [previous/old state number, whether to update the table html]
    async serverReq<A extends State>(stream: DebugStream<A>, snum: number): Promise<[number, string | undefined]> {
        let cached_step = stream.cached_states.get(snum);
        // If both the step and the table were cached, use those.
        if ((cached_step !== undefined) && (stream.cachedTable !== undefined)) {
                let old_num = stream.cur_state.num;
                stream.cur_state = cached_step;
                return [old_num, undefined];
        }       

        if (mm0Client.client === undefined) {
            throw new Error("No valid client in requestSteps")
        }
        let [old_meta, old_num] = [this.meta, stream.cur_state.num];
        let get_table = (stream.cachedTable === undefined) ? true : false;
        let unify = (stream.identify === ActiveStream.Unify) ? true : false;

        let rust_response = await 
            mm0Client
            .client
            .sendRequest(
            '$/mmbDebugger/InfoByName',
            { file_uri: this.fileUri, decl_ident: old_meta.decl_ident, elab_level: this.elabLevel, bracket_level: this.bracketLevel, unify_req: unify, stepnum: snum, table: get_table }
        ).catch((error) => {
            let error_text = 'mm0-rs failed to produce a debugging context. The language server received the following error: ' + error;
            throw new Error(error_text)
        });

        let response = rust_response as ServerResponse<A>;

        if (response.error != undefined) {
            throw new Error(`response error: ${response.error}`)
        }

        if (response.meta === undefined) {
            console.log('RESPONSE CAST FAILED: NO META');
            throw new Error("Cast into ServerResponse failed")
        }
        this.meta = response.meta;

        // This should only change in the event that a reload/mmb file change has happened, 
        // in which case we want the newer meta.
        if (response.meta.decl_ident !== old_meta.decl_ident) {
            console.log(`old ident: ${old_meta.decl_ident}`);
            console.log(`new ident: ${response.meta.decl_ident}`);
            throw new Error(`Idents didn't match: ${old_meta.decl_ident} != ${response.meta.decl_ident}`)
        }    

        for (let state of response.states) {
            //if (try_proof_state(state) === undefined) {
            //    throw new Error(`Failed to convert proof state from JSON response`)
            //}
            stream.cached_states = stream.cached_states.set(state.num, state);
        }
        let fetched = stream.cached_states.get(snum);
        if (fetched === undefined) {
            throw new Error(`Couldn't get step ${snum} step for ${stream.identify} even though server request succeeded.`)
        } else {
            stream.cur_state = fetched
        }

        // If we didn't have a cached table, we should have received one
        if (response.table != undefined) {
            stream.cachedTable = response.table;
            return [old_num, response.table]
        } else {
            return [old_num, undefined]
        }
    }

    async changeElabLevel(snum: number, force_table: boolean, new_level: number, argstream?: ProofStream | UnifyStream): Promise<void> {
        if (this.elabLevel === new_level) {
            return;
        } else {
            let old_level = this.elabLevel;
            this.elabLevel = new_level;
            this.clearCachedSteps();
            await this.trySetState(snum, force_table, argstream).catch((e) => {
                this.elabLevel = old_level;
                showError(e);
            });
            return;
        }
    }

    async changeBracketLevel(snum: number, force_table: boolean, argstream?: ProofStream | UnifyStream): Promise<void> {
        let old_level = this.bracketLevel;
        this.bracketLevel = (this.bracketLevel + 1) % 2;
        this.clearCachedSteps();
        await this.trySetState(snum, force_table, argstream).catch((e) => {
            this.bracketLevel = old_level;
            showError(e);
        });
        console.log(`changed bracket level to ${this.bracketLevel}`)
        return;
    }

    // ** We don't want to just use the current active stream, because this function
    // is fallible and the user may be trying to switch from one stream to another. 
    async trySetState(snum: number, force_table: boolean, argstream?: ProofStream | UnifyStream): Promise<void> {
        // If we didn't explicitly pass a stream, use the active one since 
        // we're not preparing for a view switch
        let stream = (argstream === undefined) ? this.activeStream() : argstream;
        if ((snum < 0) || (snum > this.totalSteps(stream.identify))) {
            return
        }

        let [old_num, table_html] = await this.serverReq(stream, snum).catch((e) => { 
            console.log('bad req');
            throw new Error(`trySetState -> tryGetState error: ${e}`)
        });
        this.active = stream.identify;
        this.updateWebview(old_num, force_table, table_html);
        return;
    }	


    totalSteps(discrim: ActiveStream): number {
        switch (discrim) {
            case ActiveStream.Proof: 
                return this.meta.total_proof_steps
            case ActiveStream.Unify: 
                return this.meta.total_unify_steps
        }
    }

    // We've already completed the requests successfully and set the new states
    // at this point.
    updateWebview(old_num: number, force_table: boolean, table_html: string | undefined) {
        if (this.stateView !== undefined) {
            let view_body = this.activeStream().htmlBody(this.meta);
            let html = this.mkWebviewContents(view_body, this.activeStream().cur_state.num, themes.defaultUserCss);
            this.stateView.webview.html = html;
        }

        // If the table webview is open
        if (this.tableView !== undefined) {
            let cached_table = this.activeStream().cachedTable;
            if (table_html !== undefined) {
                let table_body = tableBody(this.meta, this.activeStream(), table_html);
                let table = this.mkWebviewContents(table_body, this.activeStream().cur_state.num, themes.defaultUserCss);
                this.tableView.webview.html = table;
            } else if (force_table && cached_table !== undefined) {
                let table_body = tableBody(this.meta, this.activeStream(), cached_table);
                let table = this.mkWebviewContents(table_body, this.activeStream().cur_state.num, themes.defaultUserCss);
                this.tableView.webview.html = table;
            }
            this.tableView.webview.postMessage({ 
                command: 'table_move', 
                new: `${this.activeStream().cur_state.num}`, 
                old: `${old_num}`
            });

        }
        return;
    }	


    mkWebviewContents(body: string, table_active: number, user_css: themes.UserCss): string {
        return `<!DOCTYPE html>
        <html lang="en">
        <head>
            <meta charset="UTF-8">
            <meta name="viewport" content="width=device-width, initial-scale=1.0">
            <title>mmb debugger</title>
            <style>
            table {
                font-family: arial, sans-serif;
                border-collapse: collapse;
                width: 100%;
            }
            tborder {
                border: 1px solid ${user_css.red()};
            }
            td, th {
                border: 1px solid ${user_css.yellow()};
                text-align: left;
                padding: 8px;
            }
            tr {
                /*color: ${user_css.base16Colors[0]};*/
                background-color: ${user_css.base16Colors[1]};
            }
            .var {
                color:${user_css.yellow()};
            }
            .bvar {
                color:${user_css.cyan()};
            }
            .dummy {
                color:${user_css.red()};
            }
            .sort {
                color:${user_css.green()};
            }
            .term {
                color:${user_css.magenta()};
            }
            .def {
                color:${user_css.cyan()};
            }
            .thm {
                color:${user_css.violet()};
            }
            .ax {
                color:${user_css.red()};
            }
            .verif_grid {
              display: grid;
              width: 100%;
              height: 800px;
              grid-template-columns: 33% 33% 33%;
              grid-template-rows: 50% 50%;
              padding: 5px;
              grid-column-gap: 10px;
              row-gap: 0px;
            }
            .stack_item {
                border-top: 2px dotted;
                border-top-color: ${user_css.base16Colors[5]};
                color: ${user_css.base16Colors[6]};
                /*border: 3px gray;
                border-style: outset;*/
            }
            .heap_item {
                border-bottom: 2px dotted;
                border-bottom-color: ${user_css.base16Colors[5]};
                color: ${user_css.base16Colors[5]};
                /*border: 3px gray;
                border-style: outset;*/
            }
            .stack {
              border-bottom: 1px solid;
              border-bottom-color: ${user_css.base16Colors[4]};
              padding-left: 10px;
              padding-right: 10px;
              padding-bottom: 0px;
              padding-top: 5px;
              text-align: left;
              position: relative;
              display: flex;
              flex-direction: column-reverse;
              overflow: auto;
              overflow-x: auto;
              overflow-y: auto;
              margin: 0px 0px 0px 0px;
            }
            .heap {
              border-top: 1px solid;
              border-top-color: ${user_css.base16Colors[4]};
              padding-left: 10px;
              padding-right: 10px;
              padding-bottom: 5px;
              padding-top: 0px;
              text-align: left;
              position: relative;
              display: flex;
              flex-direction: column;
              overflow-x: auto;
              overflow-y: auto;
              margin: 0px 0px 0px 0px;
            }
            ul {
              list-style: none;
            }    
            .outer_flex {
              display: flex;
              background-color: ${user_css.base16Colors[1]};
              flex-direction: column;
              height: 100%;
              width: 100%;
              margin-top: 0px;
              margin-bottom: 0px;
              margin-left: 0px;
              margin-right: 0px;
              padding-top: 10px;
              padding-bottom: 10px;
              padding-left: 10px;
              padding-right: 10px;
            }
            .flex-container {
              display: flex;
              color: ${user_css.base16Colors[6]};
              border-top: 1px solid;
              border-top-color: ${user_css.base16Colors[6]};
              flex-direction: column;
              height: 100%;
              width: 100%;
              margin: 0px 0px 0px 0px;
              padding: 0px 0px 0px 0px;
            }
            .flex-container-sticky {
              display: flex;
              color: ${user_css.base16Colors[6]};
              background-color: ${user_css.base16Colors[1]};
              border-top: 1px solid;
              border-top-color: ${user_css.base16Colors[6]};
              flex-direction: column;
              height: 100%;
              width: 100%;
              margin: 0px 0px 0px 0px;
              padding-top: 5px;
              padding-bottom: 5px;
              padding-left: 0px;
              padding-right: 0px;
              position: sticky;
              bottom: 0;
            }
            .flex-container > div {
              margin: 0px 0px 0px 0px;
              padding: 0px 0px 0px 0px;
              /*margin: 10px;
              padding: 20px;*/
            }
            .use_editor_font {
                font-size: var(--vscode-editor-font-size);
                font-weight: var(--vscode-editor-font-weight);
                font-family: var(--vscode-editor-font-family);
            }
            input[type=number] {
                width: 10%;
                background-color: ${user_css.base16Colors[1]};
                color: ${user_css.base16Colors[5]};
                height: 100%;
                padding: 0px;
                margin: 0px;
                border: 1px solid ${user_css.base16Colors[5]};
                border-radius: 4px;
            }
            </style>
        </head>
        <body>
            ${body}

            <script>


                (function() {
                    const vscode = acquireVsCodeApi();
                    
                    document.getElementById('skip_to_form').onsubmit = function() {
                        vscode.postMessage({
                            command: 'skip_to',
                            text: document.getElementById('skip_to_val').value
                        })
                        return;
                    }

                    window.addEventListener('message', event => {

                        const message = event.data; 
                           switch (message.command) {
                            case 'table_move' :
                                // Setting the new one needs to be after removing the style from the old one
                                // so that when switching views you still get highlighting, because old is equal to new.
                                document.getElementById('r' + message.old).style.backgroundColor = "${user_css.base16Colors[1]}";
                                document.getElementById('n' + message.old).style.color = "${user_css.base16Colors[5]}";
                                document.getElementById('r' + message.new).style.backgroundColor = "${user_css.base16Colors[3]}";
                                document.getElementById('n' + message.new).style.color = "${user_css.red()}";
                                break;
                        }
                    });

                    document.addEventListener('keydown', takestep);
                    function takestep(e) {
                        switch (e.key) {
                            case 'ArrowRight': case 'ArrowDown': case 'j': case 'l':
                                vscode.postMessage({
                                    command: 'step_fwd',
                                    text: e.key
                                })
                                return;
                            case 'ArrowLeft': case 'ArrowUp': case 'h': case 'k':
                                vscode.postMessage({
                                    command: 'step_back',
                                    text: e.key
                                })
                                return;
                            case 'r':
                                vscode.postMessage({
                                    command: 'reload',
                                    text: e.key
                                })
                                return;
                            case 'u':
                                vscode.postMessage({
                                    command: 'viewUnify',
                                    text: e.key
                                })
                                return;
                            case 'p':
                                vscode.postMessage({
                                    command: 'viewProof',
                                    text: e.key
                                })
                                return;
                            case 't':
                                vscode.postMessage({
                                    command: 'viewTable',
                                    text: e.key
                                })
                                return;
                            case 's':
                                vscode.postMessage({
                                    command: 'viewState',
                                    text: e.key
                                })
                                return;
                            case '0':
                                vscode.postMessage({
                                    command: 'elabLevel0',
                                    text: e.key
                                })
                                return;
                            case '1':
                                vscode.postMessage({
                                    command: 'elabLevel1',
                                    text: e.key
                                })
                                return;
                            case '2':
                                vscode.postMessage({
                                    command: 'elabLevel2',
                                    text: e.key
                                })
                                return;
                            case 'b':
                                vscode.postMessage({
                                    command: 'bracketLevel',
                                    text: e.key
                                })
                                return;
                            default:
                                vscode.postMessage({
                                    command: 'do_nothing',
                                    text: 'unrecognized key'
                                })
                                return;
                        }
                    }
                }())
            </script>		

        </body>
        </html>`;
    }
}

export async function debuggerWebview(context: vc.ExtensionContext, client: LanguageClient): Promise<MmbDebugView | undefined> {
    let active_editor = vc.window.activeTextEditor;
    if (active_editor === undefined) {
        throw new Error("### no active text editor")
    } 

    let decl_ident = await infoviewDropdown();
    if (decl_ident === undefined) return undefined;
    let base = new MmbDebugView(context, decl_ident, client.code2ProtocolConverter.asUri(active_editor.document.uri));

    // If the initial request fails, just return the error, otherwise open the webview.
    base.openViewIfClosed(Wview.State, 0).catch((e) => { 
        showError(e)
        throw e 
    })
    return base
}
