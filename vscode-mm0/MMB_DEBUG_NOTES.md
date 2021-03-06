
## Basics:

When the editor is focused on an mm1 file which has a corresponding mmb file in the same path, open the vscode command palette, and use the command `debug by ident`, which should be pretty discoverable thanks to the fuzzy auto-complete thing.

The command will prompt you for the ident/name of the declaration you want to debug. If users search for a name that doesn't appear in the mmb index, they'll get an error pop-up letting them know.

The layout for the debugger is:

```
Stack, ustack, hstack
heap, uheap, vars
---
Info
```

The vars section is currently unused (see the Variable Info heading below).

Navigate through the proof steps using either arrow keys or vim controls (hjkl).
left/h: step back one
right/l: step forward one
up/k: step forward 10
down/j: step down 10
r: reload, flushing the cached steps and getting new info from the server. Useful if the mmb file has changed.

The jump-to input field will let you jump to an arbitrary step number.

The steps are 0-indexed so that the displayed number matches the 'next step' position in the proof iterator. 0/71 means the next step is the 0th element of the proof iterator, and the displayed state is the state BEFORE execution of that proof step.

The colors for the syntax highlighting are intended to be definable by users in their vscode config. Any scheme with 16 colors will satisfy the CSS class, but the choice of colors sort of assumes a base16 theme. The css styling for the s-expressions is done on the rust side since the syntactic information would be a pain to recover after the mmb items have been turned into s-expressions.
The vscode extension API only has access to a little bit of the theming information for the user's theme in webviews, which is why (IMO) using base16 is preferrable, since the same palette can be laid out as either a dark or light theme.

## Things that depend on/might depend on future changes to mm0-rs

### Variable info: 
A previous iteration of this got variable names by parsing the mm1 AST, but since the variable info will eventually be in the MMB index, that seems preferrable for a number of reasons, so that's waiting to be implemented.


### Automatic compilation
The debugger watches the vscode workspace's open folders for changes, and reloads when any of the files with a `mm*` extension are changed. This isn't super useful right now since a change in the mm1 file doesn't automatically trigger compilation of the corresponding mmb file. This is something that should be worked out; it may not always be desirable for the default behavior to be "overwrite the existing mmb file for every new change".


### A notion of 'workspace'
If the extension was aware of a user-defined workspace that included a set of files, searches wouldn't have to be tied to things like 'active editor'.
Right now you also need to either prompt the user for the mmb filepath explicitly, or you need to make an assumption about naming schemes and file locations of the mmb file w.r.t. the mm1 file. The latter is probably not going to work well in the long run since an mm1 project with more than one mm1 file tied together by imports only produces a single mmb file, which obviously can't share a name with all of the mm1 files. In the interim, you could do something like check each `<name>.mmb` in the import graph, but that seems like a lot of work for something that's not a great long term solution.

### Errors:
To use this for debugging bad proofs, the mmb compiler will need to be able to produce files that can still be parsed to some extent even in the presence of errors. This might be easier if user's are required to explicilty end a bad/incomplete proof with `sorry` before debugging, and the mmb/mmbd file has something like a `sorry` instruction used only in the debug file.

CSS stuff that needs work:
- For some reason, `overflow-x: scroll` doesn't work in the `<ul>` tags when rendered by vscode's webview; the same html and css in the webview here works fine when overflowing in Firefox. I'm not sure whether users would prefer having separating lines and no overflow (as it is now), or whether they would prefer one item per line and have the x-overflow scroll.

- In a similar vein, I couldn't get the `resize` CSS class to work in a way that allows users to resize the collections; the other ones wouldn't resize in response, and end up overlapping.

- It would be nice if the collections (stack, heap, ustack, etc.) had semi-transparent labels in the corner.

## TODOS:
+ Once the variable information (types and names) is made available through the MMB index, print that by default, and add the variable numbers in the "vars" display.

+ Allow user to chooe elaboration level: 
  - Level 0: raw term names, variable numbers, s-expressions, etc.
  - Level 1: add variable names
  - Level 2: Use notation 

The output is probably much easier to read with notation enabled, but that makes the displayed information dependent on both the mm1 parser, and the printing process for the notation.

