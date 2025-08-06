# tale

A tail-compatible tool for pretty-printing ndjson files, especially logs.

All I wanted was a newline-delimited json log pretty-printer, and they wouldn't give it to me. Then this happened. It's not yet finished in two senses. First, there are some tail features that aren't done yet. Also, it's unfinished in that edge cases aren't well handled, and memory use might explode with very large files and very large negative offsets. If all you want to do is cat or tail a log file with ndjson content, it'll do a nice job with constant modest memory use.

## Usage

```
> tale --help
A tail-compatible tool for pretty-printing ndjson files, especially logs.

Tale displays the colorfully-formatted contents of FILE, by default stdin,
to stdout. It highlights the fields likely to appear in log lines for servers,
such as level or severity, the log message, timestamps, and so on. It also
displays every field that shows up in the log line, using the color theme you
have set in your terminal.

Lines that are invalid json are printed intact, without formatting.

Tale can also follow and display more than one file at a time, with header
decoration options like `tail`'s.

Usage: tale [OPTIONS] [ARGS]...

Arguments:
  [ARGS]...
          Arguments: (offset) [file ...] where offset can be +N, -N, or N

Options:
  -f, --follow
          Follow the file, continuing to watch for more data to arrive

  -F, --sticky
          Follow the file, also checking to see if has been renamed or has an
          new inode number. If the input file does not exist yet, wait and
          display it from the beginning if and when it is created

  -b, --blocks <BLOCKS>
          Start tailing the input offset by ±N blocks

  -c, --bytes <BYTES>
          Start tailing the input offset by ±N bytes; e.g., to skip garbage

  -n, --offset <OFFSET>
          Start tailing the input offset by ±N lines

  -v, --verbose
          When following more than one file, show a header with the file name
          along with every line from that file. Not yet implemented

  -q, --quiet
          Do not ever show file name headers when following more than one file.
          Not yet implemented

  -t, --timestamps
          Show timestamps, which are hidden by default

      --window <WINDOW>
          Batch window size for multi-file tailing (in milliseconds)
          [default: 250]

      --chunked
          Force use of chunked file processing for better memory efficiency on large files

      --no-chunked
          Disable chunked file processing and always use streaming (might use more memory)

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

I have no plans to do any of the other `tail` options. Uh. Other than multi-file tailing. And some more offset types. And the quiet/verbose… I give in.

## Notes

On my Macbook `tale` will pretty-print a million-line file at an approx rate of 387K lines/sec using just under 4MB of memory, steady. Claude Code wrote benchmark tools including a test data generator; find them in the [benches](./benches) directory. I should try to source real-world data to test with, however, because I think they will provide better results. Real-world data is way more consistent than the test data is. Tale is CPU-bound on json deserialization, and it is fastest when deserializing into well-understood logging patterns.

To that point: If there is a specific json logging pattern you use that `tale` does not support directly, please give me some samples-- anonymized if you prefer-- and I'll implement deserialization and pretty-printing specifically for that pattern.

There is fairly stupid single-pass layout approach to print the key/value pairs I don't have an opinion about in columns. There is hand-tweaked formatting for the keys I do have an opinion about.

File reading is somewhat pathological (aka not good) when offsets are very large for very large files. That is, if you say `tale -500000 rilly-long.log` and the file has 500,001 lines, nothing smart will happen. You probably get what you deserve, to be honest. At least memory use won't explode. There's similarly bad behavior for very large byte and block offsets with `stdin`, because I haven't yet implemented falling back to tempfiles when I hit certain size thresholds.

I have [ripline](https://lib.rs/crates/ripline) in my back pocket for when I start tailing multiple files at once and being I/O bound instead of CPU bound is even remotely possible.

## LICENSE

This code is licensed via [the Parity Public License.](https://paritylicense.com) This license requires people who build on top of this source code to share their work with the community, too. This means if you hack on it for work, you have to make your work repo public somehow. Fair's fair. See the license text for details.
