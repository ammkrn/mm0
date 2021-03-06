import * as vc from 'vscode';
import { debuggerWebview } from './infoview';
import { Mm0Client } from './mm0_client';


//let infoView: InfoView
export const client: Mm0Client = new Mm0Client()

export function activate(context: vc.ExtensionContext) {


	// Unfortunately it is not possible to set the default line endings to LF,
	// which is required for MM0 files. Instead we just try to set it to LF
	// on open and save.
	function makeLF(doc: vc.TextDocument) {
		if (doc.languageId === 'metamath-zero' &&
				doc.eol !== vc.EndOfLine.LF &&
				vc.window.activeTextEditor) {
			vc.window.activeTextEditor.edit(
				builder => builder.setEndOfLine(vc.EndOfLine.LF))
		}
	}
	context.subscriptions.push(
		vc.workspace.onDidOpenTextDocument(makeLF),
		vc.workspace.onWillSaveTextDocument(e => makeLF(e.document)),
		vc.commands.registerCommand('metamath-zero.restartServer', () => client.restart()),
		vc.commands.registerCommand('metamath-zero.shutdownServer', () => client.stop().then(() => {}, () => {})),
		vc.commands.registerCommand('metamath-zero.debugByIdent', async () => {
			if (client.client === undefined) {
				throw new Error("No valid client")
			}
			await debuggerWebview(context, client.client)
			.catch((error) => {
				vc.window.showErrorMessage(`${error}`);
			})
		})
	);

	client.start();
}

export function deactivate(): Thenable<void> | undefined {
	if (!client) {
		return undefined;
	}
	return client.stop();
}




