# `memlimit`: A process memory limiter
This program allows you to kill a process that exceeds a provided amount of memory consumed by a process.

## Usage example
`memlimit -c 16GB cargo install ripgrep --force`

This command will:
- Follow all children (`-c`)
- With an upper limit of 16GB (16×1000³ bytes)
- Monitor the command `cargo install ripgrep --force`, and kill it if the consumed memory exceeds the upper limit.

The `memlimit` command will exit with the same exit code as the invoked process, including when it is killed (which will probably be a non-zero value, depending on the OS).

### Available flags
- `-c`, `--children`: Instead of only monitoring the process spawned by the passed command, `memlimit` will monitor every single process in the "family tree" of the original spawned process, and use the sum of the amounts of consumed memory of all children to enforce the limit.
- `--virtual`: Instead of monitoring *resident set size* memory (i.e. actual amount of memory consumed by a process), `memlimit` will use *virtual memory* values. (Note: the shorthand version of the flag, which would be `-v`, isn't allowed because it might be confused for a (nonexistant) "verbose" flag or the version flag).

### Upper limit format
The format for the amount of memory to use as the upper limit is the following:
- A single number, e.g. `300`
- A number with the `B` suffix, essentially the same as the above: `300B`
- A number followed by a SI unit of information, either with the decimal (e.g. `300MB` = 300×1000² bytes) or binary (e.g. `300MiB` = 300×1024² bytes) meanings.

No whitespace between the number and the unit is allowed.

If the resulting amount of memory is greater than the maximum possible size of memory on the current architecture (i.e. 2^32 on 32-bit architectures and 2^64 on 64-bit architectures), `memlimit` will show an error:

```
$ # On a 64-bit computer
$ memlimit 15EiB echo hello
hello

$ memlimit 16EiB echo hello
error: invalid value '16EiB' for '<AMOUNT>': amount '16EiB' too big for current architecture
```