import * as vc from 'vscode';
import { debuggerWebview } from './debugger/infoview';
import { Mm0Client } from './mm0_client';

export const mm0Client: Mm0Client = new Mm0Client()

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
		vc.commands.registerCommand('metamath-zero.restartServer', () => mm0Client.restart()),
		vc.commands.registerCommand('metamath-zero.shutdownServer', () => mm0Client.stop().then(() => {}, () => {})),
		vc.commands.registerCommand('metamath-zero.debugByIdent', async () => {
			if (mm0Client.client === undefined) {
				throw new Error("No valid client")
			}
            // The "last" catch/throw is in debuggerWebview; at this point apparently we lose
            // the ability to properly display the error to users.
			await debuggerWebview(context, mm0Client.client);
		})
	);

	mm0Client.start();
}

export function deactivate(): Thenable<void> | undefined {
	if (!mm0Client) {
		return undefined;
	}
	return mm0Client.stop();
}




