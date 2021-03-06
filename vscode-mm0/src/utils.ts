import * as vc from 'vscode';

export function assert(condition: () => boolean): void {
	if (!condition()) {
		const msg = `Assert failed: "${condition.toString()}" must be true, but was not!`;
		console.error(msg);
		throw new Error(msg);
	}
}

// Getter for a map K |-> [V]
export function seqmap_set<K, V>(m: Map<K, V[]>, k: K, v: V): Map<K, V[]> {
	let got = m.get(k);
	if (got == undefined) {
		m.set(k, [v]);
	} else {
		got.push(v);
		m.set(k, got);
	}
	return m
}

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
