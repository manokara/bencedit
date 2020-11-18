# Commands

A complete list of commands used in the interactive mode. An `<argument>` argument is mandatory, while an `[argument]` argument is optional.

Command arguments can be escaped in quotes in order to capture spaces. e.g. `command-name foo bar` has two arguments while `command-name "foo bar"` has one.

`selector` arguments use bencode-rs' select syntax, where:

- `.key`: Selects key `key` in a dictionary
- `[n]`: Selects index `n` in a list.
- You combine the elements described above to select a value. e.g. `.foo` selects `foo` in the root dictionary. `.foo[1].bar` selects `foo` in the root, then index `1` from list `foo`, then key `bar` in dictionary `foo[1]`.

`value` arguments are JSON strings, which are easy to work with. They will be converted to bencode values if they are valid, according to these conditions:

- The JSON string cannot have nulls or floats. There are no such types in bencode.
- Dictionary keys are always strings.

Use escaped quotes (`\"`) to use strings in the argument, e.g. `{\"foo\": \"bar\"}`.

"Container" values are dictionaries and lists, while "primitive" values are integers, strings and bytes.

## append `<selector>` `<value>`

Adds `value` to the end of the list at `selector`.

### Errors

This command may error if:

- `selector` is malformed.
- The value at `selector` doesn't exist.
- The value at `selector` is not a list.
- `value` is invalid (as described in the preface).

## clear `[selector]`

Removes all elements if the value at `selector` (or root if there's no argument) is a container, and sets it to the default value if it is a primitive. Strings and bytes become empty, and integers are set to 0.

### Errors

This command may error if:

- `selector` is malformed.
- The value at `selector` doesn't exist.

## insert `<selector>` `<identifier>` `<value>`

Inserts `value`into the container described by `selector` at `identifier`. `identifier` is either a key or an index, which is determined by the container type.

### Errors

This command may error if:

- `selector` is malformed.
- The value at `selector` doesn't exist.
- The value at `selector` is not a container.
- `identifier` is not a valid integer when used on lists.
- `identifier`, as an index, is out of bounds for the list.
- `value` is invalid (as described in the preface).

## reload

Reloads the current file. It will prompt you to continue if there are changes that were not saved, but will not save them automatically.

### Errors

This command may error if there were any I/O errors in the process, such as file not existing anymore, permission errors, etc.

## remove `<selector>`

Removes the value at `selector` from its parent container. Selector may point to an element in a list or a value in a dictionary.

### Errors

This command may error if:

- `selector` is malformed.
- The value at `selector` doesn't exist.

## save

Saves any changes to the structure to its original path, if there are any.

### Errors

This command may error if there were any I/O errors in the process, such as file not existing anymore, permission errors, etc.

## save-as `<path>`

Saves the structure to another file located at `path`. It will prompt you to overwrite in case the path already exists.

### Errors

This command may error if there were any I/O errors in the process, such as parent folder not existing, permission errors, etc.

## set `<selector>` `<value>`

Sets the value at `selector` to `value`.

### Errors

This command may error if:

- `selector` is malformed.
- The value at `selector` doesn't exist.
- The value at `selector` is not a container.
- `value` is invalid (as described in the preface).

## show `[selector]`

Show the contents of the value at `selector`. With no arguments, it shows the root value.

### Errors

This command may error if:

- `selector` is malformed.
- The value at `selector` doesn't exist.

## quit

Closes the editor.

### Aliases

`q`, `exit`.
