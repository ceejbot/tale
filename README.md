# tale

All I wanted was a newline-delimited json log pretty-printer, and they wouldn't give it to me.

## Usage

```
> tale --help
A tool for pretty-printing json logs or any ndjson content that has a message, a level, and a
timestamp.

The timestamp field may be named `time`, `ts`, or `timestamp`. The message field may be named
`message` or `msg`. The tool has some opinions about ordering for fields commonly found in server
log structures, but will print out every field that shows up in the log line, using the color theme
you have set in your terminal.

Usage: tale [OPTIONS] [OFFSET] [FILE]

Arguments:
  [OFFSET]
          If prefixed with -, the number of lines from the end to start reading from. If prefixed
          with +, the number of lines from the start. Only makes sense if you're tailing a file
          [default: 0]

  [FILE]
          Pretty-print the named file; defaults to printing stdin if not provided
          [default: ]

Options:
  -t, --timestamps
          Show timestamps, which are hidden by default
  -f, --follow
          Follow the file, continuing to watch for more data to arrive
  -h, --help
          Print help (see a summary with '-h')
  -V, --version
          Print version
```

The tail `-f` option is not yet implemented. I have no plans to do any of the other `tail` options.

## Notes

The `-offset` option needs to be implemented via regex or a custom validator instead of how it's done currently.

I'm going to have to completely rewrite layout to do it by hand instead of using an existing
package, I think. I have too many opinions. (Columns are not the right paradigm.)

Its behavior is probably pathological (aka not good) when offsets are very large for very large files. That is, if you say `tale -500000 rilly-long.log` and the file has 500,001 lines, nothing smart will happen. You probably get what you deserve, to be honest. At least memory use won't explode.

Consider [ripline](https://lib.rs/crates/ripline).



## LICENSE

This code is licensed via [the Parity Public License.](https://paritylicense.com) This license requires people who build on top of this source code to share their work with the community, too. This means if you hack on it for work, you have to make your work repo public somehow. Fair's fair. See the license text for details.
