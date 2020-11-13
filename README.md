# bencedit

Bencedit can edit files interactively from the command line or process several files in batch mode. It is still very WIP right now and only has a couple of simple commands.

## Usage

```
bencedit [FLAGS] [OPTIONS] <files>...

FLAGS:
    -b, --batch             Process several files through transforms
    -h, --help              Prints help information
    -S, --skip-invalid      In batch mode, skip invalid files
    -N, --skip-not-found    In batch mode, skip non-existant files
    -V, --version           Prints version information

OPTIONS:
    -t, --transform <transform>...    An action to apply to files in batch mode

ARGS:
    <files>...
```

See the [command list] and the [transform list].

[command list]: docs/commands.md
[transform list]: docs/transforms.md
