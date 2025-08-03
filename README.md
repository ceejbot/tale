# tale

A tail-compatible tool for pretty-printing ndjson files, especially logs.

All I wanted was a newline-delimited json log pretty-printer, and they wouldn't give it to me. Then this happened.

## Usage

```
> tale --help
A tail-compatible tool for pretty-printing ndjson files, especially logs.

It displays the colorfully-formatted contents of FILE, by default stdin, to stdout. Tale highlights
the fields likely to appear in log lines for servers, such as level or severity, the log message,
timestamps, and so on. It also displays every field that shows up in the log line,  using the color
theme you have set in your terminal.

Lines that are invalid json are printed intact, without formatting.

`tail` can also follow and display more than one file at a time, with header decoration options like
`tail`'s.

Usage: tale [OPTIONS] --blocks <BLOCKS> --bytes <BYTES> --offset <OFFSET> [ARGS]...

Arguments:
  [ARGS]...
          Arguments: [offset] [file] or [file1] [file2] ... for multi-file mode

Options:
  -t, --timestamps
          Show timestamps, which are hidden by default
  -f, --follow
          Follow the file, continuing to watch for more data to arrive
  -F, --sticky
          Follow the file, also checking to see if has been renamed or has an new inode number. If
          the file does not exist yet, wait and display it from the beginning if and when it is
          created
  -b, --blocks <BLOCKS>
          Start tailing offset by N blocks.  Not yet respected
  -c, --bytes <BYTES>
          Start tailing offset by N bytes; e.g., to skip garbage.  Not yet respected
  -n, --offset <OFFSET>
          Start tailing offset by N lines. Not yet respected
  -v, --verbose
          When following more than one file, show a header with the file name along with every line
          from that file.  Not yet respected
  -q, --quiet
          Do not ever show file name headers when following more than one file
      --window <WINDOW>
          Batch window size for multi-file tailing (in milliseconds)
          [default: 250]
  -h, --help
          Print help (see a summary with '-h')
  -V, --version
          Print version
```

I have no plans to do any of the other `tail` options. Uh. Other than multi-file tailing. And some more offset types. And the quiet/verbose... I give in.

## Notes

On my Macbook `tale` will pretty-print a million-line file at an approx rate of 387K lines/sec using just under 4MB of memory, steady. Claude Code wrote benchmark tools including a test data generator; find them in the [benches](./benches) directory. I should try to source real-world data to test with, however.

There is fairly stupid single-pass layout approach to print the key/value pairs I don't have an opinion about in columns. There's hand-tweaked formatting for the keys I do have an opinion about. If you are using a log format that looks a lot like canonical logs but in json, it's very efficient and readable.

File reading is probably pathological (aka not good) when offsets are very large for very large files. That is, if you say `tale -500000 rilly-long.log` and the file has 500,001 lines, nothing smart will happen. You probably get what you deserve, to be honest. At least memory use won't explode.

I have [ripline](https://lib.rs/crates/ripline) in my back pocket for when I start tailing multiple files at once and being I/O bound instead of CPU bound is even remotely possible.

## LICENSE

This code is licensed via [the Parity Public License.](https://paritylicense.com) This license requires people who build on top of this source code to share their work with the community, too. This means if you hack on it for work, you have to make your work repo public somehow. Fair's fair. See the license text for details.
