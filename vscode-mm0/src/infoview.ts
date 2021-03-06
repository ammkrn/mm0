import * as vc from 'vscode';
import { client } from './extension';
import { infoviewDropdown } from './utils';
import { LanguageClient } from 'vscode-languageclient';
import * as themes from './themes';


class DebuggerMetadata {
	decl_ident: string;
	styled_decl_ident: string;
	decl_kind: string;
	decl_num: number;
	num_steps_offset: number;

    constructor(
		decl_kind: string,
		decl_num: number,
		decl_ident: string,
    	styled_decl_ident: string,
		num_steps_offset: number,
	) {
		this.decl_kind = decl_kind;
		this.decl_num = decl_num;
		this.decl_ident = decl_ident;
		this.styled_decl_ident = styled_decl_ident;
		this.num_steps_offset = num_steps_offset;
    }
}

const InitialMeta = new DebuggerMetadata("loading...", 0, "loading...", "loading...", 0);

class DebuggerState {
    stepnum: number;
    stack: string[];
    heap: string[];
    ustack: string[];
    uheap: string[];
    hstack: string[];
    vars: string[];
    cmd: string;
	// for unification
    tgt: string;
    finish: string;

	constructor(
        stepnum: number,
        stack: string[],
        heap: string[],
        ustack: string[],
        uheap: string[],
        hstack: string[],
        vars: string[],
        cmd: string,
        tgt: string,
        finish: string
	) {
		this.stepnum = stepnum;
		this.stack= stack;
		this.heap= heap;
		this.ustack= ustack;
		this.uheap = uheap;
		this.hstack  = hstack;
		this.vars  = vars;
		this.cmd = cmd;
		this.tgt = tgt;
		this.finish= finish;
	}
}

const InitialState = new DebuggerState(
	0, 
	["loading..."], 
	["loading..."], 
	["loading..."], 
	["loading..."], 
	["loading..."], 
	["loading..."], 
	"loading...",
	"loading...",
	"loading..."
);

class DebuggerResponse {
	metadata: DebuggerMetadata | undefined;
	states: DebuggerState[] | undefined;
	// Currently unused since the mmb file won't parse if there's a proof error.
	error: string | undefined;
}



export class MmbDebugView {
    private view: vc.WebviewPanel | undefined
	private metadata: DebuggerMetadata;
	private displayed_stepnum: number;
    private cached_states: Map<number, DebuggerState>;
	private context: vc.ExtensionContext;
	// The lsp Uri type is a string. Don't keep converting it.
	private fileUri: string | undefined;
	private mm1FileWatcher: vc.FileSystemWatcher

    private isClosed(): boolean {
        return !this.view
    }

	// the `r` key in the debugger will reload debugger after flushing
	// the cached steps so that any changes in the mmb file will be reflected.
	private async reload(n?: number) {
		console.log('reloaded debugger')
		this.cached_states = new Map<number, DebuggerState>();
		if (n === undefined) {
			await this.move_to(this.displayed_stepnum)
		} else {
			await this.move_to(n)
		}

	}

	// Try to move to step `n` and update the webview accordingly.
	// 1. If the desired step isn't cached, make a request for more.
	// 2. update the tracker for the currently displayed state number (used for tracking fwd/back movement)
	// 3. update the webview using `getWebviewContent()`
	private async move_to(n: number) {
		//console.log(`move to ${n}`)

		if (this.view === undefined) {
			throw new Error("View was undefined in `move_to()`")
		}

		let this_step = this.cached_states.get(n);
		if (this_step === undefined) {
			this_step = await this.requestSteps(n).catch((err) => { throw err });
		}

		this.displayed_stepnum = n;
        this.view.webview.html = getWebviewContent(this.metadata, this_step, `${n}/${this.metadata.num_steps_offset}`, themes.defaultUserCss);
	}

	// returns the `start_at` step while also fetching `start_at` +- some 
	// number of steps, which get cached.
	async requestSteps(start_at: number): Promise<DebuggerState> {
		// Get the cursor position so we can tell the server what to look for.
		// Make the initial request for some states
		if (this.fileUri === undefined) {
	        throw new Error("no cached fileUri in requestSteps")
		}
		if (client.client === undefined) {
			throw new Error("No valid client in requestSteps")
		}

		// The ident should match even after the new request; other stuff may or may not change 
		// if the mmb file changed.
		let old_ident = this.metadata.decl_ident;

		if (client.client === undefined) {
			throw new Error("No valid client in requestSteps")
		}

		let rust_response = await 
		    client
		    .client
			.sendRequest(
			'mmbDebugger/InfoByName',
			{ use_var_names: false, stepnum: start_at, file_uri: this.fileUri, decl_ident: this.metadata.decl_ident, parse_only: false }
		).catch((error) => {
			let error_text = 'mm0-rs failed to produce a debugging context. The language server received the following error: ' + error;
			throw new Error(error_text)
		});

		let response = rust_response as DebuggerResponse;
		if ((response.metadata === undefined) || (response.states === undefined)) {
			console.log('CAST FAILED');
			throw new Error("Cast into DebuggerResponse failed")
		}

		if (response.metadata.decl_ident != old_ident) {
			console.log(`old ident: ${old_ident}`);
			console.log(`new ident: ${response.metadata.decl_ident}`);
			throw new Error(`Idents didn't match: ${old_ident} != ${response.metadata.decl_ident}`)
		}

		this.metadata = response.metadata;
		for (let state of response.states) {
			this.cached_states = this.cached_states.set(state.stepnum, state);
		}
		let return_val = this.cached_states.get(start_at);
		if (return_val === undefined) {
			throw new Error("Couldn't get nth step even though server request succeeded.")
		}
		return return_val;
	}

    async openViewIfClosed(): Promise<void> {
        //console.log('### OPEN VIEW');
        if (!this.isClosed()) {
            return
        }

		// Create the webview
        this.view = vc.window.createWebviewPanel(
			'metamath-zero mmb debugger', 
			`debug ${this.metadata.decl_ident}`,
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

		// Populate new webview with the data from step 0
		await this.move_to(0);

		// This handles messages sent from the scripts embedded in the webview,
		// which is how we get navigation keystrokes and skip_to commands.
		//
		// This is the nicest way I found to get keyboard inputs specific to a particular
		// window; I think a where context would bleed through if you had more than one 
		// debugger webview open simultaneously.
		this.view.webview.onDidReceiveMessage(
			message => {
			 switch (message.command) {
			  case 'step_fwd':
			    //vc.window.showErrorMessage('fwd: ' + message.text);
				if (this.displayed_stepnum == this.metadata.num_steps_offset) {
					// If we were already at the last step, do nothing.
					return
				} else {
			        this.move_to(this.displayed_stepnum + 1);
				}
			    return;
			  case 'step_fwd_10':
				if ((this.displayed_stepnum + 10) > this.metadata.num_steps_offset) {
					// If we were already at the last step, do nothing.
					this.move_to(this.metadata.num_steps_offset)
				} else {
			        this.move_to(this.displayed_stepnum + 10);
				}
			    return;
			  case 'step_back':
			    //vc.window.showErrorMessage('back: ' + message.text);
				if (this.displayed_stepnum == 0) {
					// If we were at the beginning, do nothing.
					return
				} else {
					this.move_to(this.displayed_stepnum - 1);
				}
			    return;
			  case 'step_back_10':
			    //vc.window.showErrorMessage('back: ' + message.text);
				if (this.displayed_stepnum < 10) {
					this.move_to(0);
				} else {
					this.move_to(this.displayed_stepnum - 10);
				}
			    return;
			  case 'reload':
				this.reload(this.displayed_stepnum);
				// Even though there are other situations in which we reload (file changes), 
				// only show the pop-up if they explicitly request the reload so the user 
				// positive feedback that they got what they asked for.
				vc.window.showInformationMessage(`reloaded_ ${this.fileUri}`);
			    return;
			  case 'skip_to':
			    //vc.window.showErrorMessage('skip_to: ' + message.text);
				let parsed = parseInt(message.text, 10);
				if (parsed > this.metadata.num_steps_offset) {
					// This shouldn't be possible since the html limits the input to the max number
					// of steps, but if we do get a larger number somehow, just move to the last step.
			        this.move_to(this.metadata.num_steps_offset);
				} else {
			        this.move_to(parsed);
				}
			    return;
			  default:
			    //vc.window.showErrorMessage('default' + message.text);
				// Some unmapped key; do nothing.
				return;
			 }
			},
			undefined,
			this.context.subscriptions
		);




        this.view.onDidDispose(() => {
            //console.log('######### disposed webview');
            this.view = undefined
        })
    }

    constructor(context: vc.ExtensionContext, decl_ident: string, fileUri: string) {
		this.metadata = InitialMeta;
		this.metadata.decl_ident = decl_ident;
		this.fileUri = fileUri;
		this.displayed_stepnum = 0;
		this.cached_states = new Map<number, DebuggerState>();
		this.context = context;

		// Reload the debugger with new contents in the event of any metamath file changes.
        this.mm1FileWatcher = vc.workspace.createFileSystemWatcher('**/*.mm*', false, false, false);
		this.mm1FileWatcher.onDidChange(() => {
			this.reload()
		})
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
	await base.requestSteps(0).catch((err) => { throw err });
	base.openViewIfClosed()
	return base
}

function makeStackItem(s: string) {
    return `<li class="stack_item">${s}</li>\n`
}

function makeHeapItem(s: string) {
    return `<li class="heap_item">${s}</li>\n`
}

function makeStack(l: string[]) {
	return l.reduce((sink, next) => { return (sink + makeStackItem(next)) }, "")
}

function makeHeap(l: string[]) {
	return l.reduce((sink, next) => { return (sink + makeHeapItem(next)) }, "")
}

export function getWebviewContent(meta: DebuggerMetadata, state: DebuggerState, step_counter: string, user_css: themes.userCss) {
	return `<!DOCTYPE html>
	<html lang="en">
	<head>
		<meta charset="UTF-8">
		<meta name="viewport" content="width=device-width, initial-scale=1.0">
		<title>Cat Coding</title>
		<style>
		.bvar {
			color:${user_css.cyan()};
		}
		.var {
			color:${user_css.yellow()};
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
			color:${user_css.green()};
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
            color: ${user_css.violet()};
            /*border: 3px gray;
            border-style: outset;*/
        }
		.heap_item {
            border-bottom: 2px dotted;
			border-bottom-color: ${user_css.base16Colors[5]};
            color: ${user_css.violet()};
            /*border: 3px gray;
            border-style: outset;*/
        }
		.stack {
		  border-bottom: 1px solid;
		  border-bottom-color: ${user_css.cyan()};
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
		  border-top-color: ${user_css.cyan()};
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
		  color: ${user_css.base16Colors[5]};
		  border-top: 1px solid;
		  border-top-color: ${user_css.base16Colors[5]};
		  flex-direction: column;
		  height: 100%;
		  width: 100%;
		  margin: 0px 0px 0px 0px;
		  padding: 0px 0px 0px 0px;
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
	    <div class="outer_flex">

		    <div class="verif_grid use_editor_font">
		    	<ul class="stack"> ${makeStack(state.stack)} </ul>
		    	<ul class="stack"> ${makeStack(state.ustack)} </ul>
		    	<ul class="stack"> ${makeStack(state.hstack)} </ul>
		    	<ul class="heap"> ${makeHeap(state.heap)} </ul>
		    	<ul class="heap"> ${makeHeap(state.uheap)} </ul>
		    	<ul class="heap"> ${makeHeap(state.vars)} </ul>
		    </div> 
		    <div class="flex-container use_editor_font">
		        <div> Verifying: ${meta.styled_decl_ident} </div>
		        <div> Next step: ${state.cmd} </div>
		        <div> Step: ${step_counter} </div>
		        <div>
		          <form id="skip_to_form">
		              <label for="jumpto">Jump to: </label>
		              <input type="number" id="skip_to_val" min="0" max="${meta.num_steps_offset}">
		          </form>
		        </div>
		    </div>

		</div>

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

		        document.addEventListener('keydown', takestep);
		        function takestep(e) {
					switch (e.key) {
						case 'ArrowRight':
		        	        vscode.postMessage({
		        	        	command: 'step_fwd',
		        	        	text: e.key
		        	        })
							return;
						case 'ArrowUp':
		        	        vscode.postMessage({
		        	        	command: 'step_fwd_10',
		        	        	text: e.key
		        	        })
							return;
						case 'k':
		        	        vscode.postMessage({
		        	        	command: 'step_fwd_10',
		        	        	text: e.key
		        	        })
							return;
						case 'l':
		        	        vscode.postMessage({
		        	        	command: 'step_fwd',
		        	        	text: e.key
		        	        })
							return;
						case 'ArrowLeft':
		        	        vscode.postMessage({
		        	        	command: 'step_back',
		        	        	text: e.key
		        	        })
							return;
						case 'h':
		        	        vscode.postMessage({
		        	        	command: 'step_back',
		        	        	text: e.key
		        	        })
							return;
						case 'ArrowDown':
		        	        vscode.postMessage({
		        	        	command: 'step_back_10',
		        	        	text: e.key
		        	        })
							return;
						case 'j':
		        	        vscode.postMessage({
		        	        	command: 'step_back_10',
		        	        	text: e.key
		        	        })
							return;
						case 'r':
		        	        vscode.postMessage({
		        	        	command: 'reload',
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

