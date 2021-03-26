import * as vc from 'vscode';
import {
    LanguageClient,
    LanguageClientOptions,
    ServerOptions
} from 'vscode-languageclient'

// Utility class for the LanguageClient that lets us add our own methods
// as desired; this setup is lifted from the lean4 extension modified to work
// with `strict: true`. IMO it's probably better in the long run to use 
// the architecture that rust-analyzer uses, which is more complex but more robust.
export class Mm0Client {
    client: LanguageClient | undefined;

    start(): Promise<void> {
        return this.restart()
    }

    async restart(): Promise<void> {
        let config = vc.workspace.getConfiguration('metamath-zero');
        let mm0Path: string = config.get('executablePath') || 'mm0-rs';
    
        // If there was already a client, wait for it to stop before continuing.
        if (this.client !== undefined) {
            await this.stop().catch((e) => { throw new Error(`couldn't stop mm0Client in 'restart': ${e}`) })
        }

        // If the extension is launched in debug mode then the debug server options are used
	    // Otherwise the run options are used
	    let serverOptions: ServerOptions = {
	    	run: { command: mm0Path, args: ['server'] },
	    	debug: { command: mm0Path, args: ['server', '--debug'] }
	    };

	    // Options to control the language client
	    let clientOptions: LanguageClientOptions = {
	    	// Register the server for MM0 files
	    	documentSelector: [{ scheme: 'file', language: 'metamath-zero' }],
	    	initializationOptions: { extraCapabilities: { goalView: true } }
	    };
        
        this.client = new LanguageClient(
		    'metamath-zero', 
            'Metamath Zero Server', 
            serverOptions, 
            clientOptions
        );
        this.client.start()
    }

    isStarted(): boolean {
        return this.client !== undefined
    }

    async stop(): Promise<void> {
        if (this.client === undefined) {
            throw new Error("Error stopping mm0 client; does not exist")
        }
        await this.client.stop();
        this.client = undefined
    }
}

