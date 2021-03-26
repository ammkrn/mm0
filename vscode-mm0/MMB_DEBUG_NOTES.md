## Basics:

When the editor is focused on an mm1 file which has a corresponding mmb file in the same path, open the vscode command palette, and use the command `debug by ident`, which should be pretty discoverable thanks to the fuzzy auto-complete thing.

The command will prompt you for the ident/name of the declaration you want to debug. If users search for a name that doesn't appear in the mmb index, they'll get an error pop-up letting them know.

The layout for the debugger is:

```
| Stack | ustack | hstack |
| heap  | uheap  |  vars  |
--
Info
```

Navigate through the proof steps using either arrow keys or vim controls (hjkl).
left/h: step back one
right/l: step forward one
up/k and down/j are now just fwd/back; it's more intuitive for the table layout.
r: reload, flushing the cached steps and getting new info from the server. Useful if the mmb file has changed.
s: view the state tab (default)
t: view the table tab
p: switches the open tab(s) to the proof stream. (default)
u: switches the open tab(s) to the unify stream.
0: use elab level 0; no notation, no variable names (though the mapping to variable names will still be available, they just won't be printed in the expressions)
1: use elab level 1; no notation
2: use elab level 2 (default)
b: cycle through levels of bracketing (gets more aggressive with displaying parentheses). Right now there are only two levels which are basically 'none' and 'the important ones'.

The jump-to input field will let you jump to an arbitrary step number.

The displayed state is the state immediately BEFORE execution of the visible step; the steps are 0-indexed. 0/71 means the next step is the 0th element of the proof iterator, and the displayed state is the state BEFORE execution of that proof step.

The colors for the syntax highlighting are intended to be definable by users in their vscode config. Any scheme with 16 colors will satisfy the CSS class, but the choice of colors sort of assumes a base16 theme. The css styling for the s-expressions is done on the rust side since the syntactic information would be a pain to recover after the mmb items have been turned into s-expressions.
The vscode extension API only has access to a little bit of the theming information for the user's theme in webviews, which is why (IMO) using base16 is preferrable, since the same palette can be laid out as either a dark or light theme.

## Things that depend on/might depend on future changes to mm0-rs

### Automatic compilation
The debugger watches the vscode workspace's open folders for changes, and reloads when any of the files with a `mm*` extension are changed. This isn't super useful right now since a change in the mm1 file doesn't automatically trigger compilation of the corresponding mmb file. This is something that should be worked out; it may not always be desirable for the default behavior to be "overwrite the existing mmb file for every new change".


### A notion of 'workspace'
If the extension was aware of a user-defined workspace that included a set of files, searches wouldn't have to be tied to things like 'active editor'.
Right now you also need to either prompt the user for the mmb filepath explicitly, or you need to make an assumption about naming schemes and file locations of the mmb file w.r.t. the mm1 file. The latter is probably not going to work well in the long run since an mm1 project with more than one mm1 file tied together by imports only produces a single mmb file, which obviously can't share a name with all of the mm1 files. In the interim, you could do something like check each `<name>.mmb` in the import graph, but that seems like a lot of work for something that's not a great long term solution.

### Errors:
To use this for debugging bad proofs, the mmb compiler will need to be able to produce files that can still be parsed to some extent even in the presence of errors. This might be easier if user's are required to explicilty end a bad/incomplete proof with `sorry` before debugging, and the mmb/mmbd file has something like a `sorry` instruction used only in the debug file.



## Needs work:
The focus method doesn't seem to work in VScode webviews, to the table view doesn't move to/focus on the step you're on.

I couldn't get the `resize` CSS class to work in a way that allows users to resize the collections; the other ones wouldn't resize in response, and end up overlapping.

The way the table works internally isn't ideal; I wanted users to be able to scroll freely through the whole thing (which works), but as a consequence, rendering the table requires rendering a (potentially) very large html document. In the extreme case this might be a deal breaker, though it's not a huge issue now. The highlighting doens't break everything because it selects the two elements of interest by id which reloads lazily. If there's a clever way of creating the illusion of scrolling while having pagination, let me know.

## Design:
When the extension needs to get info from the server, it asks for a chunk of verifier/unifier states. When it wants to know state 100, it gets the range 50-150, caches them, and then just displays state 100.
The problem with just getting a single step at a time is that you would either have to run the verifier on the proof every single time, or if you make the verifier lazy such that it's movable in 1-step increments, you would need to store the state of the verifier between extension requests, which is not a desirable option in the case where there are multiple files open, users may change the file/recompile, or an error might interrupt communication between the server and the extension.

The other extreme of sending the entire set of verifier and unifier states at once doesn't scale well, since the number of proof steps can get arbitrarily large, and even though verification is extremely fast, that doesn't mean that the process of laying the state history out in JSON format and sending it to the extension is going to be fast (it isn't). 

So the pluses/minuses of the chunking approach:

+ No need to store intermediate states of the verifier between requests
+ Debugger interactions have no noticeable delay
+ Efficient in the case that a user only wants to debug a small section, like the end of a proof

- you have to re-run the proof every time you want a new chunk. 

### Extension internals:

There are two webviews (state and table) and two streams (proof and unify). Each stream has both a state and table view.
As long as the debugger is open, both streams exist, but there can be either one or two webviews open (only state, only table, or both). When both webviews are closed, the whole debugger object is disposed of.

Before opening the webview, the extension makes an initial request for the first chunk of states. If the user requested a non-existent declaration, the mmb file doesn't exist/can't be found, or there's some error with the server, you don't get stuck with an open but blank webview (more importantly the user's editor doesn't lose focus).

Once the webview is open and populated with the initial set of states, the controlling class `MmbDebugView` contains both the proof stream and debug stream. When the user switches between proof/unify view, it just changes the html that's currently displayed in the webview.

If you're interested in making changes or improvements to the extension, one thing to keep in mind is that you don't want to change anything about the view state (either the html or MmbDebugView.active) until any fallible operations have returned successfully. If you change any of the view states THEN do some fallible operation, you run the risk of the extension and the user's view being out of sync, which will almost certainly require them to restart the extension and close some tabs to get everything working again.

Rather than making the ProofStream/UnifyStream hold the stack/heap/etc individually as arrays of strings, they receive the state as one big string that has the proper html formatting from the rust server. The extension doesn't really need to be able to unilaterally manipulate each collection and its items individually, but the 'blob of html' approach has two big advantages:

1. The extension doesn't have to worry (and do an undefined check for) each individual json field when receiving a response. This also means that name changes only have to be reflected in the rust implementation, rather than coordinated across both
2. The rust part only has to allocate one string for each collection, instead of allocating a new string for every item in the collection, which is much more efficient.


### The table:
This is tricky without paginating the table or doing some fancier lazy rendering; right now the whole table is grabbed once when loading a proof, as opposed to the states which are done in chunks. The base problem is that you want the user to freely scroll through all of the steps, but you might have a really large blob of HTML. A big proof right now still loads pretty quickly (like `mapnth`), but in the extreme case where you had like 50k steps you might run into issues. Stepping through the proof is fine, since the highlighting update is done by a query selector, so it only reloads the two DOM nodes that are being changed.
The actual string that is the table as HTML is cached, so it's only loaded once, and then reloaded in the event that the file changes or the user wants a hard reload.

One of the compromises is that you can view both the proof and unify streams by switching in the tabs that are already open, but this forces you to re-render the table every time you switch.

There are two separate parts that take time that need to be considered; one is getting the table from the server, and the other is rendering the HTML you receive from the server in the webview. Because of the size, both of them can potentially be issues.



