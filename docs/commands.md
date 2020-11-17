# Commands

## reload

Reloads the current file.

## save

Saves any changes to the structure to its original path, if there are any.

## save-as `<path>`

Saves the structure to another file located at `path`.

## set `<selector>` `<value>`

Sets the value at `<selector>` to `<value>`, which is a JSON string. It will be converted to bencode and stored in place. The command marks the current structure as changed, making a `*` appear in the command prompt (like `bencedit *>`).

## show `<selector>`

Show the contents of the value at `<selector>` from the root.

## quit

Closes the editor.
