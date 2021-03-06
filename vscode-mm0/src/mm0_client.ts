import {
    LanguageClient,
    LanguageClientOptions,
    ServerOptions
} from 'vscode-languageclient'
import { assert } from './utils';
import * as vc from 'vscode';


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
    
        if (this.isStarted()) {
            await this.stop()
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
        assert(() => this.isStarted())
        if (this.client === undefined) {
            throw new Error("No valid language client in mm0Client")
        }
        await this.client.stop();
        this.client = undefined
    }
}

